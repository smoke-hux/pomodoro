mod domain;
mod notifications;
mod quiet;
mod sound;
mod storage;
mod system;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicI8, AtomicU8, Ordering},
        Arc, Condvar, Mutex, OnceLock, PoisonError,
    },
    thread,
    time::Duration,
};

use chrono::Utc;
use domain::{AppData, CaptureStatus, InterruptionCategory, Phase, Settings, TimerStatus};
use notifications::NotificationListener;
use sound::Cue;
use storage::Store;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_notification::NotificationExt;

/// Captured notifications are persisted on a timer rather than one write per
/// notification. A burst — a group chat waking up, a build reporting every step
/// — would otherwise rewrite the whole JSON store many times a second, on the
/// monitoring thread, while the user is trying to focus. User actions still save
/// immediately; only observed notifications wait.
const NOTIFICATION_FLUSH_MS: i64 = 2_000;

/// How long to leave a store that would not save before trying it again.
///
/// A write that failed was retried at once: the dirty mark stayed where it was,
/// already overdue, so [`wait_for`] answered zero and the reconciliation thread
/// went straight round again — one core at full load, the data lock held almost
/// without a break, and the same line written to stderr as fast as it would
/// go, for as long as the disk stayed full. A disk that is full now is full in
/// ten milliseconds too. Half a minute is soon enough to pick up the space
/// somebody freed and slow enough to cost nothing.
const SAVE_RETRY_MS: i64 = 30_000;

struct RuntimeState {
    data: Mutex<AppData>,
    store: Store,
    listener: Arc<NotificationListener>,
    /// The time from which unsaved changes are counted as waiting, or 0 when
    /// nothing is unsaved. A write is due [`NOTIFICATION_FLUSH_MS`] after it.
    ///
    /// Normally that is when the oldest unsaved captured notification arrived.
    /// After a failed save it is set ahead of the clock by [`retry_mark`], so
    /// the next attempt comes due [`SAVE_RETRY_MS`] later; [`wait_for`] and
    /// [`RuntimeState::flush_notifications`] both read "due" off this one
    /// value, and so cannot disagree about when that is.
    notifications_dirty_since: AtomicI64,
    /// The last banner state [`sync_quiet`] acted on: `1` quiet, `0` normal,
    /// `-1` not yet decided. Asking the desktop what its banner setting is
    /// means spawning a process; this makes that happen once per transition
    /// rather than once per wake-up.
    quiet_desire: AtomicI8,
    /// What the reconciliation thread sleeps on. Anything that changes the
    /// timer or files a notification calls [`Self::nudge`] so the thread
    /// re-evaluates immediately instead of at its next scheduled wake-up.
    ///
    /// The lock holds "nudged since the thread last looked". It used to hold
    /// nothing, and a nudge was only a `notify_all`: one that arrived after
    /// the thread had worked out how long to sleep but before it was asleep
    /// woke nobody and was gone. Resume a timer with five seconds left in that
    /// gap and the thread slept its full idle half-minute; the phase ended
    /// twenty-five seconds late. A flag set under the lock is still there
    /// when the thread comes to wait.
    wake: Condvar,
    wake_lock: Mutex<bool>,
    /// Set first thing by [`Self::shut_down`] and never cleared. From then on
    /// nothing may silence the desktop and the reconciliation thread stops.
    shutting_down: AtomicBool,
    /// The tray's start/pause item and the label it currently shows, set once
    /// the tray is built. Kept so the item can say what it will do.
    tray_toggle: OnceLock<MenuItem<tauri::Wry>>,
    /// Indexes into [`TOGGLE_LABELS`]: what the item should say, recorded under
    /// the data lock, and what it says, touched only on the main thread.
    tray_wanted: AtomicU8,
    tray_shown: AtomicU8,
}

/// While a phase is counting down the thread wakes at least this often, so a
/// completion is never late by more than this even if the monotonic clock
/// stood still across a suspend.
const RUNNING_WAKE: Duration = Duration::from_secs(1);
/// With nothing counting down and nothing waiting to be written there is
/// nothing to do. A long ceiling rather than no ceiling, as a backstop.
const IDLE_WAKE: Duration = Duration::from_secs(30);

/// How long the reconciliation thread can sleep before something needs it.
///
/// Running: until the deadline, capped at [`RUNNING_WAKE`]. Idle with a
/// pending write: until that is due. Otherwise: a long time. The old loop woke
/// twice a second, all day, whether or not anything was counting down.
///
/// `dirty_since` is [`RuntimeState::notifications_dirty_since`], 0 for nothing
/// pending. Kept free of the state so the arithmetic can be tested.
fn wait_for(running_deadline: Option<i64>, dirty_since: i64, now_ms: i64) -> Duration {
    if let Some(ends_at) = running_deadline {
        let until = u64::try_from(ends_at.saturating_sub(now_ms)).unwrap_or(0);
        return Duration::from_millis(until).min(RUNNING_WAKE);
    }

    if dirty_since != 0 {
        let due = dirty_since.saturating_add(NOTIFICATION_FLUSH_MS);
        let until = u64::try_from(due.saturating_sub(now_ms)).unwrap_or(0);
        return Duration::from_millis(until).min(IDLE_WAKE);
    }

    IDLE_WAKE
}

/// The dirty mark to leave after a save failed at `now_ms`: the one that makes
/// the next write come due [`SAVE_RETRY_MS`] from now. Never 0, which would
/// read as "nothing to save" and forget the change altogether.
fn retry_mark(now_ms: i64) -> i64 {
    now_ms
        .saturating_add(SAVE_RETRY_MS - NOTIFICATION_FLUSH_MS)
        .max(1)
}

/// Whether the desktop's banners may be turned off right now.
///
/// Never once shutdown has begun. Quitting from the tray mid-focus restored
/// the banners, but the focus was still running and so was the reconciliation
/// thread, which could wake before the process was gone, find the desktop not
/// quiet during a focus, and silence it again — on its way out, with nothing
/// left alive to put it back until the next launch.
fn may_silence(want_quiet: bool, shutting_down: bool) -> bool {
    want_quiet && !shutting_down
}

impl RuntimeState {
    fn new(data: AppData, store: Store) -> Self {
        Self {
            data: Mutex::new(data),
            store,
            listener: Arc::new(NotificationListener::new()),
            notifications_dirty_since: AtomicI64::new(0),
            quiet_desire: AtomicI8::new(-1),
            wake: Condvar::new(),
            wake_lock: Mutex::new(false),
            shutting_down: AtomicBool::new(false),
            tray_toggle: OnceLock::new(),
            tray_wanted: AtomicU8::new(0),
            tray_shown: AtomicU8::new(0),
        }
    }

    /// Wakes the reconciliation thread, or, if it is not asleep yet, tells it
    /// not to go to sleep.
    fn nudge(&self) {
        // Nothing can panic while holding this lock, and a bool has no
        // invariant a panic could have broken.
        let mut nudged = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *nudged = true;
        self.wake.notify_all();
    }

    /// Sleeps until nudged or until `timeout` has passed, whichever is first.
    /// A nudge delivered since the last call counts: the wait is skipped.
    ///
    /// Waking early for no reason would be harmless — the caller recomputes
    /// everything — but `wait_timeout_while` does not do that either.
    fn wait_for_work(&self, timeout: Duration) {
        let mut nudged = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !*nudged {
            nudged = match self
                .wake
                .wait_timeout_while(nudged, timeout, |nudged| !*nudged)
            {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        // Cleared before the lock goes, so a nudge from here on is a new one.
        *nudged = false;
    }

    /// How long the reconciliation thread can sleep; see [`wait_for`].
    fn next_wait(&self, now_ms: i64) -> Duration {
        let running_deadline = self
            .data
            .lock()
            .ok()
            .filter(|data| data.timer.status == TimerStatus::Running)
            .and_then(|data| data.timer.ends_at);
        wait_for(
            running_deadline,
            self.notifications_dirty_since.load(Ordering::Acquire),
            now_ms,
        )
    }
}

impl RuntimeState {
    /// Notes that captured notifications are waiting to be written.
    fn mark_notifications_dirty(&self, now_ms: i64) {
        let _ = self.notifications_dirty_since.compare_exchange(
            0,
            now_ms.max(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn clear_notifications_dirty(&self) {
        self.notifications_dirty_since.store(0, Ordering::Release);
    }

    /// Writes the store, and keeps the dirty mark honest about the result:
    /// cleared by a write that worked, since that covers everything including
    /// captured notifications; pushed out by [`SAVE_RETRY_MS`] after one that
    /// did not, so the change is neither forgotten nor retried in a spin.
    ///
    /// Takes `data` to prove the data lock is held. The mark only changes
    /// under it, so a failed write here cannot re-arm a mark that a later,
    /// successful write on another thread has just cleared.
    ///
    /// A store closed to writing answers `Ok`, and so is never retried.
    fn save(&self, data: &AppData, now_ms: i64) -> Result<(), String> {
        match self.store.save(data) {
            Ok(()) => {
                self.clear_notifications_dirty();
                Ok(())
            }
            Err(error) => {
                self.notifications_dirty_since
                    .store(retry_mark(now_ms), Ordering::Release);
                Err(error)
            }
        }
    }

    /// Writes captured notifications once they have been waiting long enough,
    /// and anything else a failed save left behind once its retry is due.
    /// Called from the reconciliation loop, so a quiet burst still lands within
    /// a couple of seconds.
    ///
    /// A failure is reported once here and not tried again for
    /// [`SAVE_RETRY_MS`]; see there for the spin this used to be.
    fn flush_notifications(&self, now_ms: i64, force: bool) {
        let dirty_since = self.notifications_dirty_since.load(Ordering::Acquire);
        if dirty_since == 0 {
            return;
        }
        if !force && now_ms.saturating_sub(dirty_since) < NOTIFICATION_FLUSH_MS {
            return;
        }
        let Ok(data) = self.data.lock() else {
            return;
        };
        if let Err(error) = self.save(&data, now_ms) {
            eprintln!(
                "could not save; trying again in {} s: {error}",
                SAVE_RETRY_MS / 1_000
            );
        }
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn lock_data(state: &RuntimeState) -> Result<std::sync::MutexGuard<'_, AppData>, String> {
    state
        .data
        .lock()
        .map_err(|_| "local timer state is unavailable".to_string())
}

/// Announces the end of a phase with a banner. The banner is silent, with no
/// sound hint: the sound is [`sound::play`]'s business alone, see there.
fn notify_boundary(app: &AppHandle, completed: Phase, snapshot: &AppData) {
    if !snapshot.settings.notifications {
        return;
    }

    let (title, body) = match completed {
        Phase::Focus => match snapshot.timer.phase {
            Phase::LongBreak => (
                "Focus cycle complete",
                "You finished four rounds. Take a restorative long break.",
            ),
            _ => ("Focus complete", "Step away for a short break."),
        },
        Phase::ShortBreak | Phase::LongBreak => (
            "Break complete",
            "Choose one task when you are ready to focus again.",
        ),
    };

    let _ = app.notification().builder().title(title).body(body).show();
}

/// The labels the tray's start/pause item can carry, indexed by
/// [`toggle_label_index`].
const TOGGLE_LABELS: [&str; 4] = ["Start focus", "Start break", "Pause", "Resume"];

/// What the tray's start/pause item will do if clicked now. It used to read
/// "Start / Pause" whatever the timer was doing, and the tray is exactly where
/// the window is not there to show which one applies.
fn toggle_label_index(status: TimerStatus, phase: Phase) -> u8 {
    match (status, phase) {
        (TimerStatus::Idle, Phase::Focus) => 0,
        (TimerStatus::Idle, Phase::ShortBreak | Phase::LongBreak) => 1,
        (TimerStatus::Running, _) => 2,
        (TimerStatus::Paused, _) => 3,
    }
}

#[cfg(test)]
fn toggle_label(status: TimerStatus, phase: Phase) -> &'static str {
    TOGGLE_LABELS[usize::from(toggle_label_index(status, phase))]
}

impl RuntimeState {
    /// Takes the snapshot to broadcast and, in the same breath, records the
    /// label the tray should carry.
    ///
    /// Must be called with the data lock held, which is what `data` proves:
    /// the record is then written in the same order the state changed in, and
    /// [`sync_tray`] can read it without the lock. Reading the state there
    /// instead made the GTK main thread wait on a mutex that other threads
    /// hold across a disk write and a `gsettings` call.
    fn snapshot(&self, data: &AppData, now_ms: i64) -> AppData {
        self.tray_wanted.store(
            toggle_label_index(data.timer.status, data.timer.phase),
            Ordering::SeqCst,
        );
        data.snapshot(now_ms)
    }
}

/// Relabels the tray item when the label needs to change.
///
/// The relabelling always runs on the main thread and takes the label from
/// the record at that moment, not from the snapshot that prompted it. Two
/// threads can ask at nearly the same time — the timer thread at a phase end,
/// the main thread on a click — and if each carried its own label across, the
/// older one could land last and stay until the next transition. Run in one
/// place and read late, whichever runs last is right.
///
/// Reading late once was still not enough, which is why the relabelling is
/// [`settle_tray`]'s loop. See there for the pause-then-resume that left
/// "Resume" on a running timer.
fn sync_tray(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    if state.tray_toggle.get().is_none() {
        return;
    }
    // Most publishes are a task edit or a filed notification, not a timer
    // transition; those stop here without troubling the main thread.
    if state.tray_wanted.load(Ordering::SeqCst) == state.tray_shown.load(Ordering::SeqCst) {
        return;
    }

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<RuntimeState>();
        let Some(item) = state.tray_toggle.get() else {
            return;
        };
        settle_tray(&state.tray_wanted, &state.tray_shown, |index| {
            item.set_text(TOGGLE_LABELS[usize::from(index)]).is_ok()
        });
    });
}

/// Applies the wanted label until the label shown is the label wanted.
///
/// This used to read `wanted` once, relabel, and record what it had shown.
/// Relabelling takes time, and `shown` is only updated after it. With "Pause"
/// showing, a pause asked for "Resume" and the relabelling began; a resume
/// then asked for "Pause" again, and its publish compared that against
/// `shown` — still "Pause", the relabelling not having finished — and took
/// [`sync_tray`]'s fast path out. The relabelling then finished and recorded
/// "Resume", on a running timer, until some later publish happened to differ.
///
/// So after recording what was shown, `wanted` is read again, and a change
/// that arrived in the meantime is applied too. Only the main thread writes
/// `shown`, so the loop ends as soon as nobody is racing it. An `apply` that
/// fails ends it as well, with `shown` untouched: the next publish tries
/// again, where retrying here would spin on a tray that is not answering.
///
/// The orderings are `SeqCst` on both atomics, everywhere, because this is
/// two threads each writing one value and then reading the other. Under
/// anything weaker each may miss the other's write — the publisher sees the
/// old `shown` and leaves, the loop sees the old `wanted` and stops — which
/// is the same lost update by another road.
fn settle_tray(wanted: &AtomicU8, shown: &AtomicU8, mut apply: impl FnMut(u8) -> bool) {
    loop {
        let target = wanted.load(Ordering::SeqCst);
        if shown.load(Ordering::SeqCst) == target || !apply(target) {
            return;
        }
        shown.store(target, Ordering::SeqCst);
    }
}

/// Broadcasts the new state to the window. This is the one channel through
/// which the UI learns anything: commands do not return the state as well, so
/// each change is serialised and sent once, not twice.
fn publish(app: &AppHandle, snapshot: &AppData) {
    let _ = app.emit("state-changed", snapshot);
    sync_tray(app);
}

/// Files an observed notification, persists it and broadcasts the new state.
///
/// Runs on the monitoring thread. The notification text goes into the local
/// store and into the frontend event; it is never logged.
fn record_notification(app: &AppHandle, event: notifications::NotifyEvent) {
    let state = app.state::<RuntimeState>();
    let now = now_ms();
    let Ok(mut data) = state.data.lock() else {
        return;
    };
    if data
        .capture_notify(
            event.app_name,
            event.summary,
            event.body,
            event.urgency,
            event.replaces_id,
            now,
        )
        .is_none()
    {
        return;
    }
    let snapshot = state.snapshot(&data, now);
    drop(data);
    // The write is deferred; the UI is not. A notification appears in the inbox
    // the moment it is filed, and reaches disk within NOTIFICATION_FLUSH_MS.
    state.mark_notifications_dirty(now);
    state.nudge();
    publish(app, &snapshot);
}

/// Records the monitor's health so the UI can tell "capture is on" apart from
/// "capture is working". Runs on the monitoring thread; the caller must not hold
/// the data lock.
fn report_capture_status(app: &AppHandle, status: CaptureStatus) {
    let state = app.state::<RuntimeState>();
    let Ok(mut data) = state.data.lock() else {
        return;
    };
    if data.capture_status == status {
        return;
    }
    data.capture_status = status;
    let snapshot = state.snapshot(&data, now_ms());
    drop(data);
    publish(app, &snapshot);
}

/// Brings the desktop's banner setting in line with the current phase, and
/// reports whether the persisted restore marker moved.
///
/// The marker holds the value to put back, so an app that is killed mid-focus
/// leaves behind everything the next launch needs to repair the desktop.
///
/// Once shutdown has begun this can only ever restore; see [`may_silence`].
fn sync_quiet(state: &RuntimeState, data: &mut AppData, now_ms: i64) -> bool {
    let want_quiet = may_silence(
        data.settings.silence_banners_during_focus && data.is_focus_running(now_ms),
        state.shutting_down.load(Ordering::SeqCst),
    );
    let desire = i8::from(want_quiet);
    if state.quiet_desire.swap(desire, Ordering::AcqRel) == desire {
        return false;
    }

    match (want_quiet, data.banner_restore) {
        (true, None) => {
            // Banners the user had already switched off are not ours to claim,
            // and nothing needs restoring afterwards.
            let Some(true) = quiet::read_show_banners() else {
                return false;
            };
            if let Err(error) = quiet::write_show_banners(false) {
                eprintln!("could not silence notification banners: {error}");
                return false;
            }
            data.banner_restore = Some(true);
            true
        }
        (false, Some(previous)) => {
            if let Err(error) = quiet::write_show_banners(previous) {
                eprintln!("could not restore notification banners: {error}");
            }
            // Cleared either way: a marker that cannot be acted on would make
            // every later launch retry a write that does not work.
            data.banner_restore = None;
            true
        }
        _ => false,
    }
}

/// Brings the monitoring thread in line with the persisted filter. Idempotent,
/// so it is safe to call after any settings change.
fn sync_listener(app: &AppHandle, state: &RuntimeState, enabled: bool) {
    if !enabled {
        state.listener.stop();
        report_capture_status(app, CaptureStatus::off());
        return;
    }
    let sink_handle = app.clone();
    let status_handle = app.clone();
    state.listener.start(
        move |event| record_notification(&sink_handle, event),
        move |status| report_capture_status(&status_handle, status),
    );
}

/// What one pass over the state produced, for the caller to broadcast once
/// the data lock is released.
struct Change {
    /// The state to publish, or `None` when nothing changed.
    snapshot: Option<AppData>,
    /// The phase that ran out during this pass, to be announced.
    completed: Option<Phase>,
    /// What the caller is told: the action's error if it had one, otherwise
    /// the save's.
    result: Result<(), String>,
}

impl Change {
    fn nothing(result: Result<(), String>) -> Self {
        Self {
            snapshot: None,
            completed: None,
            result,
        }
    }
}

/// Moves the timer forward to `now`, and only if a phase completed or the
/// banners changed hands persists and returns something to broadcast. Nothing
/// is cloned or serialised on the common path where nothing happened, which is
/// nearly every wake-up.
///
/// A save that fails does not stop the broadcast. The state in memory has
/// already moved on and is the truth for this session; returning at the failed
/// save left the window at 00:00 on a phase the backend had finished, with no
/// alert, and a window reloaded at that moment got nothing at all, since
/// `get_snapshot` came through here too. The error is still returned, after
/// the window has been told, and the write is retried; see
/// [`RuntimeState::save`].
fn reconcile(state: &RuntimeState, now: i64) -> Change {
    let mut data = match lock_data(state) {
        Ok(data) => data,
        Err(error) => return Change::nothing(Err(error)),
    };
    let completed = data.tick(now);
    let quiet_changed = sync_quiet(state, &mut data, now);
    if completed.is_none() && !quiet_changed {
        return Change::nothing(Ok(()));
    }
    let result = state.save(&data, now);
    Change {
        snapshot: Some(state.snapshot(&data, now)),
        completed,
        result,
    }
}

/// Applies a user action to the state, after first settling a phase that ran
/// out while the reconciliation thread slept.
///
/// That settling is a change in its own right, and used to be lost whenever
/// the action then refused — an empty task title, a task deleted a moment
/// before. The `?` on the action left before the save, the broadcast and the
/// alert, so the finished phase existed only in memory and the window sat at
/// 00:00 until something else happened to publish. Now an action's error
/// returns early only when nothing else happened. Every action validates
/// before it touches anything, so one that refuses has left the data as the
/// tick left it, and that is what is saved and published.
///
/// When the action and the save both fail the action's error is returned: it
/// is the one the person can do something about. A failed save is handled as
/// in [`reconcile`].
fn apply<F>(state: &RuntimeState, now: i64, action: F) -> Change
where
    F: FnOnce(&mut AppData, i64) -> Result<(), String>,
{
    let mut data = match lock_data(state) {
        Ok(data) => data,
        Err(error) => return Change::nothing(Err(error)),
    };
    let completed = data.tick(now);
    let acted = action(&mut data, now);
    if acted.is_err() && completed.is_none() {
        return Change::nothing(acted);
    }
    sync_quiet(state, &mut data, now);
    let saved = state.save(&data, now);
    Change {
        snapshot: Some(state.snapshot(&data, now)),
        completed,
        result: acted.and(saved),
    }
}

/// Tells the window, and the user if a phase ended, what a pass produced, and
/// hands back the pass's result. Called with the data lock released.
///
/// The sound and the banner are two settings and neither waits on the other:
/// the sound is decided here, ahead of [`notify_boundary`], because that
/// returns early with notifications off and used to be the only alert there
/// was. `sound::play` returns at once, so this costs the reconciliation thread
/// nothing.
///
/// Only a phase that ran out while Pomodoro was watching gets here. Skipping
/// or resetting an interval completes nothing, and a phase settled at launch
/// by `recover_at_launch` is never broadcast as a completion, so both stay
/// silent: nobody wants an alarm for an interval they cut short, or for one
/// that ended while the application was closed.
fn broadcast(app: &AppHandle, change: Change) -> Result<(), String> {
    if let Some(snapshot) = &change.snapshot {
        publish(app, snapshot);
        if let Some(phase) = change.completed {
            if snapshot.settings.sound {
                sound::play(Cue::IntervalFinished);
            }
            notify_boundary(app, phase, snapshot);
        }
    }
    change.result
}

/// [`reconcile`], broadcast. What the reconciliation thread runs on every
/// wake-up.
fn advance(state: &RuntimeState, app: &AppHandle) -> Result<(), String> {
    broadcast(app, reconcile(state, now_ms()))
}

/// [`apply`], broadcast. What every command that changes anything goes through.
fn mutate<F>(state: &RuntimeState, app: &AppHandle, action: F) -> Result<(), String>
where
    F: FnOnce(&mut AppData, i64) -> Result<(), String>,
{
    let change = apply(state, now_ms(), action);
    if change.snapshot.is_some() {
        // The timer may have started, stopped or moved; the sleeping thread
        // needs to recompute how long it can wait.
        state.nudge();
    }
    broadcast(app, change)
}

fn toggle_timer_impl(state: &RuntimeState, app: &AppHandle) -> Result<(), String> {
    mutate(state, app, |data, now| {
        if data.timer.status == TimerStatus::Running {
            data.pause(now);
            return Ok(());
        }
        if data.timer.phase == Phase::Focus && data.timer.active_task_id.is_none() {
            return Err("Choose a task before starting a focus session.".to_string());
        }
        data.start_or_resume(now);
        Ok(())
    })
}

/// The one command that returns state: the window asks once, on load, and
/// follows `state-changed` from then on.
#[tauri::command]
fn get_snapshot(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    // A save that failed is not a reason to show a reloaded window nothing:
    // it needs the state more than it needs that error, which is being
    // retried in the background anyway.
    if let Err(error) = advance(&state, &app) {
        eprintln!("timer reconciliation failed: {error}");
    }
    let data = lock_data(&state)?;
    Ok(state.snapshot(&data, now_ms()))
}

#[tauri::command]
fn toggle_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    toggle_timer_impl(&state, &app)
}

#[tauri::command]
fn reset_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        data.reset(now);
        Ok(())
    })
}

#[tauri::command]
fn skip_phase(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        data.skip(now);
        Ok(())
    })
}

#[tauri::command]
fn set_phase(phase: Phase, state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        if data.timer.status != TimerStatus::Idle {
            return Err("Reset the current interval before changing timer mode.".to_string());
        }
        data.set_phase(phase, now);
        Ok(())
    })
}

#[tauri::command]
fn select_task(
    task_id: Option<String>,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        if data.timer.phase == Phase::Focus && data.timer.status != TimerStatus::Idle {
            return Err("Finish or reset the current focus before switching tasks.".to_string());
        }
        if !data.select_task(task_id) {
            return Err("That task is no longer available.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn add_task(
    title: String,
    estimate: u32,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        let task = data
            .create_task(title, estimate.clamp(1, 16), now)
            .ok_or_else(|| "Enter a task name.".to_string())?;
        if data.timer.phase == Phase::Focus
            && data.timer.status == TimerStatus::Idle
            && data.timer.active_task_id.is_none()
        {
            data.select_task(Some(task.id));
        }
        Ok(())
    })
}

#[tauri::command]
fn update_task(
    id: String,
    title: String,
    estimate: u32,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        let done = data
            .tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.done)
            .ok_or_else(|| "Task not found.".to_string())?;
        if !data.update_task(&id, title, estimate.clamp(1, 16), done, now) {
            return Err("Enter a task name.".to_string());
        }
        Ok(())
    })
}

/// What toggling a task should set off besides the change itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskAlert {
    sound: bool,
    notify: bool,
}

/// Decides [`TaskAlert`] for a task that was `was_done` and is `now_done`.
///
/// Only finishing a task is an occasion. Reopening one is a correction, often
/// of a mis-click a second earlier, and congratulating it would be wrong
/// twice. Each alert then follows its own setting and nothing else. Kept free
/// of the state because the command around it cannot run without a window.
fn task_alert(was_done: bool, now_done: bool, sound: bool, notifications: bool) -> TaskAlert {
    let finished = !was_done && now_done;
    TaskAlert {
        sound: finished && sound,
        notify: finished && notifications,
    }
}

#[tauri::command]
fn toggle_task(id: String, state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    // What the toggle turned out to be, carried out of the closure: the title
    // and the settings are only to be had under the data lock, and the alert
    // is only to be raised once that lock is gone.
    let mut toggled: Option<(TaskAlert, String)> = None;
    let result = mutate(&state, &app, |data, now| {
        let (done, title) = data
            .tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| (task.done, task.title.clone()))
            .ok_or_else(|| "Task not found.".to_string())?;
        if !data.set_task_done(&id, !done, now) {
            return Err("Task not found.".to_string());
        }
        let settings = &data.settings;
        let alert = task_alert(done, !done, settings.sound, settings.notifications);
        toggled = Some((alert, title));
        Ok(())
    });
    // `toggled` is set once the task has been marked done in memory, which is
    // what the window then shows — whether or not the save that followed
    // worked. The alert used to wait on the whole result, so a full disk meant
    // a task ticked off in silence while the finished interval in the same
    // situation was announced. Memory is the truth for the running session,
    // here as in `broadcast`.
    if let Some((alert, title)) = toggled {
        // This very call may also have been the first to notice that an
        // interval had run out, in which case `mutate` has already started the
        // interval's sound. That one wins: a chime asked for while the alarm
        // is playing is dropped, so the two never sound over each other.
        if alert.sound {
            sound::play(Cue::TaskDone);
        }
        if alert.notify {
            // The title goes in the summary, which the notification
            // specification keeps as plain text. Servers that advertise
            // body-markup read the body as markup, and a task called
            // "Fix a<b" came out mangled on the ones that do not repair it.
            let _ = app
                .notification()
                .builder()
                .title(task_complete_summary(&title))
                .show();
        }
    }
    result
}

/// The one line a "task complete" notification carries: the task's title,
/// cut short if it would not fit a banner, and never empty.
fn task_complete_summary(title: &str) -> String {
    const MOST: usize = 80;
    let title = title.trim();
    let shown: String = if title.chars().count() > MOST {
        format!("{}…", title.chars().take(MOST - 1).collect::<String>())
    } else {
        title.to_string()
    };
    if shown.is_empty() {
        "Task complete".to_string()
    } else {
        format!("Task complete: {shown}")
    }
}

/// Plays the interval sound so it can be heard from Settings before relying on
/// it: whether there is a sound at all depends on what the machine has
/// installed. Deliberately not behind the Sound setting — the button is how
/// one decides about that setting — and it changes, saves and publishes
/// nothing.
///
/// It answers only once the sound has played, or failed to, so the button can
/// say "no": the whole point of pressing it is to learn whether anything will
/// be heard. That wait is why the command is async — it runs off the main
/// thread — and it is at most the sound's length plus the player's deadline.
#[tauri::command(async)]
fn preview_sound() -> Result<(), String> {
    sound::play_and_report(Cue::IntervalFinished)
}

#[tauri::command]
fn delete_task(id: String, state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        if data.timer.active_task_id.as_deref() == Some(&id)
            && data.timer.status != TimerStatus::Idle
        {
            return Err("Finish or reset this task's active focus before deleting it.".to_string());
        }
        if !data.delete_task(&id) {
            return Err("Task not found.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn capture_interruption(
    text: String,
    category: InterruptionCategory,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        data.capture_interruption(text, category, now)
            .ok_or_else(|| "Write a short note first.".to_string())?;
        Ok(())
    })
}

#[tauri::command]
fn set_interruption_handled(
    id: String,
    handled: bool,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        let item = data
            .interruptions
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| "Interruption not found.".to_string())?;
        item.handled = handled;
        Ok(())
    })
}

#[tauri::command]
fn delete_interruption(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        if !data.delete_interruption(&id) {
            return Err("Interruption not found.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn convert_interruption_to_task(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        if !data.interruptions.iter().any(|item| item.id == id) {
            return Err("Interruption not found.".to_string());
        }
        data.convert_interruption_to_task(&id, now)
            .ok_or_else(|| "Could not create a task from this note.".to_string())?;
        Ok(())
    })
}

#[tauri::command]
fn update_settings(
    settings: Settings,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let enabled = settings.notification_filter.enabled;
    let mut applied = false;
    let result = mutate(&state, &app, |data, _| {
        data.update_settings(settings);
        applied = true;
        Ok(())
    });
    // Whenever the settings took effect in memory, saved or not. Leaving on a
    // failed save had capture switched on in the window and in the state while
    // the listener stayed stopped.
    if applied {
        sync_listener(&app, &state, enabled);
    }
    result
}

#[tauri::command]
fn triage_notification(
    id: String,
    triaged: bool,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        if !data.set_notification_triaged(&id, triaged) {
            return Err("Notification not found.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn convert_notification(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        if !data
            .notifications
            .iter()
            .any(|notification| notification.id == id)
        {
            return Err("Notification not found.".to_string());
        }
        // Idempotent: a second click returns the task the first one made rather
        // than adding a duplicate to the list.
        data.convert_notification_to_task(&id, now)
            .ok_or_else(|| "This notification has no summary to name a task.".to_string())?;
        Ok(())
    })
}

#[tauri::command]
fn delete_notification(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, move |data, _| {
        if !data.delete_notification(&id) {
            return Err("Notification not found.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn clear_notifications(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, _| {
        data.clear_notifications();
        Ok(())
    })
}

#[tauri::command]
fn clear_history(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, _| {
        data.sessions.clear();
        Ok(())
    })
}

impl RuntimeState {
    /// Leaves the machine as Pomodoro found it, and the store as a quit should
    /// leave it. Runs once; later calls do nothing, because every orderly way
    /// out now comes through here and several of them fire for one exit.
    ///
    /// In order:
    ///
    /// - Shutdown is announced before anything else, so that from the first
    ///   line nothing can silence the desktop again; see [`may_silence`].
    ///   Anything that silenced it just before did so under the data lock, and
    ///   has left its marker where the restore below will find it.
    /// - A running interval is paused. It used to be left running, deadline
    ///   and all: quit five minutes into a focus, launch the next morning, and
    ///   the first tick recorded a full completed focus, credited the task,
    ///   announced "Focus complete" and started a break. Quitting is walking
    ///   away, and what is kept is the time that was left. If the interval is
    ///   in fact already due, `pause` completes it instead, which is right.
    /// - Banners go back on if they were turned off for a focus interval.
    /// - Everything is written, captured notifications included, rather than
    ///   waiting on the debounce.
    fn shut_down(&self, now_ms: i64) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.listener.stop();
        self.quiet_desire.store(0, Ordering::Release);
        if let Ok(mut data) = self.data.lock() {
            data.pause(now_ms);
            if let Some(previous) = data.banner_restore.take() {
                if let Err(error) = quiet::write_show_banners(previous) {
                    eprintln!("could not restore notification banners: {error}");
                }
            }
            if let Err(error) = self.store.save(&data) {
                eprintln!("could not save on shutdown: {error}");
            }
        }
        self.clear_notifications_dirty();
        // The reconciliation thread has nothing left to wait for.
        self.nudge();
    }
}

fn shut_down(app: &AppHandle) {
    app.state::<RuntimeState>().shut_down(now_ms());
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Pomodoro", true, None::<&str>)?;
    let state = app.state::<RuntimeState>();
    let index = state
        .data
        .lock()
        .map(|data| toggle_label_index(data.timer.status, data.timer.phase))
        .unwrap_or(0);
    state.tray_wanted.store(index, Ordering::SeqCst);
    state.tray_shown.store(index, Ordering::SeqCst);
    let label = TOGGLE_LABELS[usize::from(index)];
    let toggle = MenuItem::with_id(app, "toggle", label, true, None::<&str>)?;
    let _ = state.tray_toggle.set(toggle.clone());
    let skip = MenuItem::with_id(app, "skip", "Skip interval", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &skip, &separator, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("Pomodoro focus timer")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "toggle" => {
                let state = app.state::<RuntimeState>();
                let _ = toggle_timer_impl(&state, app);
            }
            "skip" => {
                let state = app.state::<RuntimeState>();
                let _ = mutate(&state, app, |data, now| {
                    data.skip(now);
                    Ok(())
                });
            }
            "quit" => {
                shut_down(app);
                app.exit(0);
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let store = Store::new(data_dir);
            let launched_at = now_ms();
            let mut data = store.load_or_set_aside(launched_at);
            // A phase that ran out while the app was not running — it was
            // killed, or the machine went down — is settled here, quietly,
            // rather than by the first tick: no banner for something that
            // ended hours ago and no break starting itself in an empty room.
            let recovered = data.recover_at_launch(launched_at);
            let capture_enabled = data.settings.notification_filter.enabled;
            let state = RuntimeState::new(data, store);
            if recovered {
                if let Ok(data) = state.data.lock() {
                    if let Err(error) = state.save(&data, launched_at) {
                        eprintln!("could not save the interval settled at launch: {error}");
                    }
                }
            }
            app.manage(state);
            build_tray(app)?;

            let handle = app.handle().clone();
            sync_listener(&handle, &handle.state::<RuntimeState>(), capture_enabled);

            // If a previous run was killed while it had the desktop's banners
            // turned off, the persisted marker is still set. Reconciling once
            // here puts the desktop back before the first frame is drawn.
            if let Err(error) = advance(&handle.state::<RuntimeState>(), &handle) {
                eprintln!("timer reconciliation failed: {error}");
            }

            // Sleeps until the next deadline, the next pending write, or a
            // nudge from a command — not on a fixed half-second beat.
            //
            // It stops at shutdown, checked on both sides of the sleep: once
            // the desktop and the store have been left as they should be,
            // nothing here may touch either again.
            thread::spawn(move || loop {
                let state = handle.state::<RuntimeState>();
                if state.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                state.wait_for_work(state.next_wait(now_ms()));
                if state.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(error) = advance(&state, &handle) {
                    eprintln!("timer reconciliation failed: {error}");
                }
                state.flush_notifications(now_ms(), false);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            toggle_timer,
            reset_timer,
            skip_phase,
            set_phase,
            select_task,
            add_task,
            update_task,
            toggle_task,
            delete_task,
            capture_interruption,
            set_interruption_handled,
            delete_interruption,
            convert_interruption_to_task,
            update_settings,
            clear_history,
            triage_notification,
            convert_notification,
            delete_notification,
            clear_notifications,
            preview_sound,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Pomodoro")
        // Cleanup used to hang off the tray's Quit item alone, so any other
        // orderly way out left the banners off and the last captured
        // notifications unwritten. Both events, because which of them a given
        // exit produces is the runtime's business; `shut_down` runs once
        // whichever comes first. The exit is never prevented here.
        //
        // Not covered: SIGTERM and the like end the process without either
        // event. That needs a signal handler and is dealt with separately;
        // until then the persisted restore marker repairs the desktop at the
        // next launch.
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shut_down(app);
            }
        });
}

#[cfg(test)]
mod tray {
    use super::*;

    #[test]
    fn the_tray_item_names_what_a_click_will_do() {
        assert_eq!(toggle_label(TimerStatus::Running, Phase::Focus), "Pause");
        assert_eq!(
            toggle_label(TimerStatus::Running, Phase::ShortBreak),
            "Pause"
        );
        assert_eq!(toggle_label(TimerStatus::Paused, Phase::Focus), "Resume");
        assert_eq!(toggle_label(TimerStatus::Idle, Phase::Focus), "Start focus");
        assert_eq!(
            toggle_label(TimerStatus::Idle, Phase::ShortBreak),
            "Start break"
        );
        assert_eq!(
            toggle_label(TimerStatus::Idle, Phase::LongBreak),
            "Start break"
        );
    }

    /// The pause-then-resume race, with the resume landing while the first
    /// relabelling is still under way — after its publish has already compared
    /// against the old `shown` and left.
    #[test]
    fn a_label_wanted_during_a_relabelling_is_applied_too() {
        let wanted = AtomicU8::new(3);
        let shown = AtomicU8::new(2);
        let mut applied = Vec::new();

        settle_tray(&wanted, &shown, |index| {
            if applied.is_empty() {
                wanted.store(2, Ordering::SeqCst);
            }
            applied.push(index);
            true
        });

        assert_eq!(applied, [3, 2], "the final label is applied last");
        assert_eq!(
            shown.load(Ordering::SeqCst),
            wanted.load(Ordering::SeqCst),
            "the tray ends on what is wanted"
        );
    }

    #[test]
    fn a_tray_that_will_not_relabel_is_asked_once_and_not_recorded() {
        let wanted = AtomicU8::new(3);
        let shown = AtomicU8::new(2);
        let mut asked = 0;

        settle_tray(&wanted, &shown, |_| {
            asked += 1;
            false
        });

        assert_eq!(asked, 1);
        // Still unequal, so the next publish gets past the fast path.
        assert_eq!(shown.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_tray_already_showing_what_is_wanted_is_left_alone() {
        let wanted = AtomicU8::new(1);
        let shown = AtomicU8::new(1);
        settle_tray(&wanted, &shown, |_| panic!("nothing to apply"));
    }
}

/// The reconciliation thread's sleep, the save path and shutdown, exercised on
/// a [`RuntimeState`] with no window behind it. Everything that needs an
/// `AppHandle` — publishing, the boundary alert — is downstream of what these
/// return.
#[cfg(test)]
mod runtime {
    use super::*;
    use std::{fs, path::PathBuf, time::Instant};

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pomodoro-runtime-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ))
    }

    /// A store whose every save fails: its data directory is a file.
    fn unwritable(name: &str) -> (Store, PathBuf) {
        let blocker = scratch(name);
        fs::write(&blocker, b"not a directory").unwrap();
        (Store::new(&blocker), blocker)
    }

    /// A focus on one task that began `elapsed_ms` before `now`.
    fn focus_begun(now: i64, elapsed_ms: i64) -> AppData {
        let mut data = AppData::default();
        let task = data.create_task("Write the report", 1, 0).unwrap();
        data.select_task(Some(task.id));
        data.start_or_resume(now - elapsed_ms);
        data
    }

    const TWENTY_SIX_MINUTES: i64 = 26 * 60 * 1_000;

    #[test]
    fn a_nudge_that_lands_before_the_wait_begins_is_not_lost() {
        let state = RuntimeState::new(AppData::default(), Store::new(scratch("nudge")));
        // The gap: the thread has decided how long to sleep, and is not yet
        // asleep.
        state.nudge();

        let began = Instant::now();
        state.wait_for_work(Duration::from_secs(5));
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "slept {:?} through a nudge",
            began.elapsed()
        );

        // One nudge is one wake-up: the next wait runs its time.
        let began = Instant::now();
        state.wait_for_work(Duration::from_millis(60));
        assert!(began.elapsed() >= Duration::from_millis(60));
    }

    #[test]
    fn a_nudge_during_the_wait_ends_it() {
        let state = Arc::new(RuntimeState::new(
            AppData::default(),
            Store::new(scratch("nudge-during")),
        ));
        let nudger = Arc::clone(&state);
        let began = Instant::now();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            nudger.nudge();
        });
        state.wait_for_work(Duration::from_secs(5));
        assert!(began.elapsed() < Duration::from_secs(1));
        handle.join().unwrap();
    }

    #[test]
    fn the_wait_is_the_nearest_thing_that_needs_doing() {
        let now = 1_000_000;
        assert_eq!(wait_for(None, 0, now), IDLE_WAKE);
        assert_eq!(
            wait_for(Some(now + 400), 0, now),
            Duration::from_millis(400)
        );
        assert_eq!(wait_for(Some(now + 60_000), 0, now), RUNNING_WAKE);
        assert_eq!(wait_for(Some(now - 5), 0, now), Duration::ZERO);
        // A notification filed half a second ago is written in a second and a
        // half.
        assert_eq!(wait_for(None, now - 500, now), Duration::from_millis(1_500));
        assert_eq!(wait_for(None, now - 10_000, now), Duration::ZERO);

        // After a failed save the next attempt is a retry away — not zero,
        // which is what an overdue mark left in place used to give.
        let retry = Duration::from_millis(SAVE_RETRY_MS as u64);
        assert_eq!(wait_for(None, retry_mark(now), now), retry.min(IDLE_WAKE));
        assert_eq!(
            wait_for(None, retry_mark(now), now + 10_000),
            Duration::from_millis(SAVE_RETRY_MS as u64 - 10_000)
        );
        assert_ne!(
            retry_mark(-SAVE_RETRY_MS),
            0,
            "0 would mean nothing to save"
        );
    }

    #[test]
    fn a_flush_that_fails_is_tried_again_later_rather_than_at_once() {
        let (store, blocker) = unwritable("flush");
        let state = RuntimeState::new(AppData::default(), store);
        let now = now_ms();
        state.mark_notifications_dirty(now - 5_000);
        assert_eq!(state.next_wait(now), Duration::ZERO, "the flush is overdue");

        state.flush_notifications(now, false);
        let wait = state.next_wait(now);
        assert!(
            wait >= Duration::from_millis(SAVE_RETRY_MS as u64).min(IDLE_WAKE),
            "a failed flush left a wait of {wait:?}"
        );
        // Still dirty, so the change is not forgotten...
        let mark = state.notifications_dirty_since.load(Ordering::Acquire);
        assert_eq!(mark, retry_mark(now));
        // ...and the flush agrees with the wait that it is not yet due: a
        // notification arriving meanwhile does not pull it forward either.
        state.mark_notifications_dirty(now + 1_000);
        state.flush_notifications(now + 1_000, false);
        assert_eq!(
            state.notifications_dirty_since.load(Ordering::Acquire),
            mark
        );

        // Once the store can be written, the retry lands and clears the mark.
        fs::remove_file(&blocker).unwrap();
        state.flush_notifications(now + SAVE_RETRY_MS, false);
        assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
        assert!(state.store.load().is_ok());
        let _ = fs::remove_dir_all(blocker);
    }

    #[test]
    fn a_later_save_that_works_clears_a_pending_retry() {
        let (store, blocker) = unwritable("clears");
        let state = RuntimeState::new(AppData::default(), store);
        let now = now_ms();
        state.mark_notifications_dirty(now - 5_000);
        state.flush_notifications(now, false);
        assert_ne!(state.notifications_dirty_since.load(Ordering::Acquire), 0);

        fs::remove_file(&blocker).unwrap();
        let change = apply(&state, now + 10, |data, now| {
            data.create_task("Anything", 1, now);
            Ok(())
        });
        assert_eq!(change.result, Ok(()));
        assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
        let _ = fs::remove_dir_all(blocker);
    }

    #[test]
    fn a_phase_that_ended_is_kept_when_the_action_then_refuses() {
        let directory = scratch("refused");
        let now = now_ms();
        let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), Store::new(&directory));

        let change = apply(&state, now, |data, now| {
            data.create_task("   ", 1, now)
                .ok_or_else(|| "Enter a task name.".to_string())?;
            Ok(())
        });

        // The person still hears that their title was refused...
        assert_eq!(change.result, Err("Enter a task name.".to_string()));
        // ...and the focus that finished is announced, shown and on disk.
        assert_eq!(change.completed, Some(Phase::Focus));
        let snapshot = change.snapshot.expect("the completed phase is published");
        assert_eq!(snapshot.timer.phase, Phase::ShortBreak);
        assert_eq!(snapshot.sessions.len(), 1);
        let saved = state.store.load().unwrap();
        assert_eq!(saved.sessions.len(), 1);
        assert_eq!(saved.tasks[0].completed_pomodoros, 1);
        assert_eq!(saved.tasks.len(), 1, "the refused task was not added");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn an_action_that_refuses_with_nothing_else_going_on_changes_nothing() {
        let directory = scratch("refused-idle");
        let state = RuntimeState::new(AppData::default(), Store::new(&directory));

        let change = apply(&state, now_ms(), |_, _| Err("No.".to_string()));

        assert_eq!(change.result, Err("No.".to_string()));
        assert!(change.snapshot.is_none());
        assert_eq!(change.completed, None);
        assert!(!directory.exists(), "nothing was saved");
    }

    #[test]
    fn a_save_that_fails_still_publishes_and_is_retried() {
        let (store, blocker) = unwritable("save-fails");
        let now = now_ms();
        let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);

        // A user action: applied in memory, published, and the error surfaced.
        let change = apply(&state, now, |data, now| {
            data.create_task("Second task", 1, now);
            Ok(())
        });
        assert!(change.result.is_err());
        assert_eq!(change.completed, Some(Phase::Focus));
        let snapshot = change
            .snapshot
            .expect("memory is the truth; it is published");
        assert_eq!(snapshot.tasks.len(), 2);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(
            state.notifications_dirty_since.load(Ordering::Acquire),
            retry_mark(now),
            "the write is owed, and will be retried"
        );

        // Both failing: the action's error is the one the person can act on.
        let (store, second_blocker) = unwritable("both-fail");
        let both = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);
        let change = apply(&both, now, |_, _| Err("Enter a task name.".to_string()));
        assert_eq!(change.result, Err("Enter a task name.".to_string()));
        assert!(change.snapshot.is_some());
        assert_ne!(both.notifications_dirty_since.load(Ordering::Acquire), 0);

        let _ = fs::remove_file(blocker);
        let _ = fs::remove_file(second_blocker);
    }

    #[test]
    fn a_phase_end_is_published_even_when_it_cannot_be_saved() {
        let (store, blocker) = unwritable("reconcile");
        let now = now_ms();
        let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);

        let change = reconcile(&state, now);
        assert!(change.result.is_err());
        assert_eq!(change.completed, Some(Phase::Focus));
        assert_eq!(
            change.snapshot.expect("published").timer.phase,
            Phase::ShortBreak
        );
        assert_eq!(
            state.notifications_dirty_since.load(Ordering::Acquire),
            retry_mark(now)
        );

        // Nothing further happened, so nothing further is attempted: a store
        // that will not save is not hammered from here either.
        let again = reconcile(&state, now + 1_000);
        assert_eq!(again.result, Ok(()));
        assert!(again.snapshot.is_none());
        let _ = fs::remove_file(blocker);
    }

    #[test]
    fn a_task_complete_notification_names_the_task_in_its_summary() {
        assert_eq!(
            task_complete_summary("  Fix a<b && c>d  "),
            "Task complete: Fix a<b && c>d"
        );
        assert_eq!(task_complete_summary("   "), "Task complete");
        let long = "x".repeat(200);
        let summary = task_complete_summary(&long);
        assert!(summary.ends_with('…'));
        assert!(summary.chars().count() <= "Task complete: ".len() + 80);
    }

    #[test]
    fn finishing_a_task_alerts_and_reopening_one_does_not() {
        let alert = |sound, notify| TaskAlert { sound, notify };
        // Finished: each alert follows its own setting, independently.
        assert_eq!(task_alert(false, true, true, true), alert(true, true));
        assert_eq!(task_alert(false, true, true, false), alert(true, false));
        assert_eq!(task_alert(false, true, false, true), alert(false, true));
        assert_eq!(task_alert(false, true, false, false), alert(false, false));
        // Reopened, or not changed at all: nothing, whatever the settings.
        assert_eq!(task_alert(true, false, true, true), alert(false, false));
        assert_eq!(task_alert(true, true, true, true), alert(false, false));
        assert_eq!(task_alert(false, false, true, true), alert(false, false));
    }

    #[test]
    fn nothing_silences_the_desktop_once_shutdown_has_begun() {
        assert!(may_silence(true, false));
        assert!(!may_silence(true, true));
        assert!(!may_silence(false, false));
        assert!(!may_silence(false, true));
    }

    #[test]
    fn quitting_pauses_a_running_interval_saves_it_and_happens_once() {
        let directory = scratch("quit");
        let now = now_ms();
        let state = RuntimeState::new(focus_begun(now, 5 * 60 * 1_000), Store::new(&directory));
        state.mark_notifications_dirty(now);

        state.shut_down(now);

        assert!(state.shutting_down.load(Ordering::SeqCst));
        assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
        let saved = state.store.load().unwrap();
        assert_eq!(saved.timer.status, TimerStatus::Paused);
        assert_eq!(saved.timer.remaining_seconds, 20 * 60);
        assert!(saved.sessions.is_empty());
        // The thread it nudged has nothing to sleep for.
        let began = Instant::now();
        state.wait_for_work(Duration::from_secs(5));
        assert!(began.elapsed() < Duration::from_secs(1));

        // Exit is reported more than once; only the first call acts.
        state.data.lock().unwrap().start_or_resume(now + 1_000);
        state.shut_down(now + 2_000);
        assert_eq!(
            state.data.lock().unwrap().timer.status,
            TimerStatus::Running,
            "a second shutdown did its work again"
        );
        let _ = fs::remove_dir_all(directory);
    }
}

/// Checks that need a real GNOME session, standing in for the manual test plan:
/// a notification sent by another application mid-focus, and the desktop going
/// quiet and coming back.
///
/// All `#[ignore]`d — a CI runner has no session bus and no notification daemon.
/// Run them on a GNOME desktop with:
///
/// ```text
/// cargo test -- --ignored --nocapture live_gnome
/// ```
#[cfg(test)]
mod live_gnome {
    use super::*;
    use domain::NotificationFilter;
    use std::{process::Command, sync::mpsc};

    /// Long enough for gnome-shell to accept a call and relay it to monitors.
    const SETTLE: Duration = Duration::from_millis(1_500);

    fn send(app_name: &str, summary: &str, body: &str) {
        let status = Command::new("notify-send")
            .arg(format!("--app-name={app_name}"))
            .arg(summary)
            .arg(body)
            .status()
            .expect("notify-send must be available");
        assert!(status.success(), "notify-send failed");
    }

    fn capturing() -> NotificationFilter {
        NotificationFilter {
            enabled: true,
            ..NotificationFilter::default()
        }
    }

    /// The test plan's first manual check, made repeatable.
    ///
    /// Starts a real focus interval, has another application send a notification
    /// through the session bus, and confirms it is filed as arriving during
    /// focus while the timer itself is untouched.
    #[test]
    #[ignore = "requires a live D-Bus session and notification daemon"]
    fn a_notification_sent_mid_focus_is_filed_without_disturbing_the_timer() {
        let now = now_ms();
        let mut data = AppData::default();
        data.settings.notification_filter = capturing();
        let task = data.create_task("Write the report", 1, now).unwrap();
        data.select_task(Some(task.id));
        data.start_or_resume(now);

        let started_at = data.timer.started_at;
        let ends_at = data.timer.ends_at;
        assert!(
            data.is_focus_running(now),
            "the focus interval must be running"
        );

        let (sender, receiver) = mpsc::channel();
        let (status_sender, status_receiver) = mpsc::channel();
        let listener = Arc::new(NotificationListener::new());
        listener.start(
            move |event| {
                let _ = sender.send(event);
            },
            move |status| {
                let _ = status_sender.send(status);
            },
        );
        thread::sleep(SETTLE);

        let summary = format!("standup-{}", std::process::id());
        send("Slack", &summary, "In five minutes");
        thread::sleep(SETTLE);
        listener.stop();

        // File whatever arrived exactly as record_notification does, at a
        // timestamp inside the interval.
        let filed = now_ms();
        let mut captured = Vec::new();
        for event in receiver.try_iter() {
            if event.summary != summary {
                continue;
            }
            if let Some(notification) = data.capture_notify(
                event.app_name,
                event.summary,
                event.body,
                event.urgency,
                event.replaces_id,
                filed,
            ) {
                captured.push(notification);
            }
        }

        // It landed, once, attributed to the focus interval.
        assert_eq!(
            captured.len(),
            1,
            "the notification must be filed exactly once"
        );
        let notification = &captured[0];
        assert_eq!(notification.app_name, "Slack");
        assert_eq!(notification.summary, summary);
        assert_eq!(notification.body, "In five minutes");
        assert!(
            notification.during_focus,
            "it arrived during a focus interval"
        );
        assert!(!notification.triaged, "it starts in the pending list");
        assert_eq!(data.notifications.len(), 1, "it is in the inbox");

        // The timer is exactly where it was. Capture never touches it.
        assert_eq!(data.timer.status, TimerStatus::Running);
        assert_eq!(data.timer.phase, Phase::Focus);
        assert_eq!(data.timer.started_at, started_at);
        assert_eq!(data.timer.ends_at, ends_at);
        assert!(data.sessions.is_empty(), "no session was ended or recorded");

        // The monitor reported itself healthy rather than merely switched on.
        let states: Vec<domain::CaptureState> = status_receiver
            .try_iter()
            .map(|status| status.state)
            .collect();
        assert!(
            states.contains(&domain::CaptureState::Active),
            "the monitor should have reported Active, saw {states:?}"
        );
    }

    /// Pomodoro's own boundary alerts travel the same bus. Turning capture on
    /// must not fill the inbox with them.
    #[test]
    #[ignore = "requires a live D-Bus session and notification daemon"]
    fn pomodoros_own_notifications_do_not_reach_the_inbox() {
        let mut data = AppData::default();
        data.settings.notification_filter = capturing();

        let (sender, receiver) = mpsc::channel();
        let listener = Arc::new(NotificationListener::new());
        listener.start(
            move |event| {
                let _ = sender.send(event);
            },
            |_| {},
        );
        thread::sleep(SETTLE);

        let marker = format!("boundary-{}", std::process::id());
        send("Pomodoro", &marker, "Step away for a short break.");
        send("Slack", &format!("other-{marker}"), "A real message");
        thread::sleep(SETTLE);
        listener.stop();

        let now = now_ms();
        for event in receiver.try_iter() {
            data.capture_notify(
                event.app_name,
                event.summary,
                event.body,
                event.urgency,
                event.replaces_id,
                now,
            );
        }

        assert!(
            data.notifications
                .iter()
                .all(|item| item.app_name != "Pomodoro"),
            "Pomodoro's own notification reached the inbox"
        );
        assert!(
            data.notifications
                .iter()
                .any(|item| item.app_name == "Slack"),
            "the other application's notification should still be captured"
        );
    }

    /// The desktop really goes quiet for the interval and really comes back.
    ///
    /// Restores whatever the machine had before, on every exit path, so running
    /// the test cannot leave the desktop silent.
    #[test]
    #[ignore = "requires a live GNOME session with gsettings"]
    fn banners_are_silenced_for_the_interval_and_restored_afterwards() {
        let original = quiet::read_show_banners().expect("GNOME's banner setting must be readable");
        // The feature deliberately does nothing when banners are already off,
        // so the check needs them on to be meaningful.
        quiet::write_show_banners(true).expect("the schema must be writable");

        let outcome = std::panic::catch_unwind(|| {
            let now = now_ms();
            let mut data = AppData::default();
            data.settings.silence_banners_during_focus = true;
            let task = data.create_task("Write the report", 1, now).unwrap();
            data.select_task(Some(task.id));
            data.start_or_resume(now);

            // Entering focus takes the desktop quiet and remembers what to undo.
            assert!(data.is_focus_running(now));
            assert_eq!(quiet::read_show_banners(), Some(true));
            quiet::write_show_banners(false).unwrap();
            data.banner_restore = Some(true);
            assert_eq!(
                quiet::read_show_banners(),
                Some(false),
                "banners should be off during focus"
            );

            // Leaving focus puts it back.
            data.pause(now + 1_000);
            assert!(!data.is_focus_running(now + 1_000));
            let previous = data.banner_restore.take().expect("a marker to restore");
            quiet::write_show_banners(previous).unwrap();
            assert_eq!(
                quiet::read_show_banners(),
                Some(true),
                "banners should be back after focus"
            );
        });

        quiet::write_show_banners(original).expect("the original setting must be restored");
        assert_eq!(quiet::read_show_banners(), Some(original));
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}

/// Measures what one state change costs at the store's retention caps, so
/// "faster" has a number attached. `#[ignore]`d: it prints, it does not assert.
/// Run with `cargo test --release -- --ignored --nocapture snapshot_cost`.
///
/// The window used to fetch this every second while the timer ran. It now
/// fetches it once on load and receives it once per change, and counts the
/// timer down locally in between.
#[cfg(test)]
mod snapshot_cost {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "prints a measurement rather than asserting"]
    fn full_store_snapshot_per_state_change() {
        let mut data = AppData::default();
        data.settings.notification_filter.enabled = true;
        // A realistic-heavy store: a year of six sessions a day, a full inbox
        // with long bodies, a busy task list.
        for index in 0..40 {
            data.create_task(format!("Task number {index} with a normal title"), 2, index);
        }
        for index in 0..2_000_i64 {
            data.sessions.push(domain::SessionRecord {
                id: format!("session-{index}"),
                phase: Phase::Focus,
                task_id: Some("task-0".to_string()),
                task_title: Some("Task number 0 with a normal title".to_string()),
                duration_seconds: 1_500,
                started_at: index * 1_800_000,
                ended_at: index * 1_800_000 + 1_500_000,
                outcome: domain::SessionOutcome::Completed,
            });
        }
        for index in 0..domain::NOTIFICATION_RETENTION as i64 {
            data.capture_notify(
                "Thunderbird",
                "x".repeat(200),
                "y".repeat(2_000),
                1,
                0,
                index,
            );
        }

        // "Now" is after the newest session, as it would be in use, so the
        // snapshot's session window is measured honestly.
        let now = data.sessions.last().map_or(0, |session| session.ended_at) + 60_000;

        let iterations: u32 = 200;
        let start = Instant::now();
        let mut bytes = 0;
        for _ in 0..iterations {
            let snapshot = data.snapshot(now);
            bytes = serde_json::to_vec(&snapshot).unwrap().len();
        }
        let per_change = start.elapsed() / iterations;
        let everything = {
            let mut whole = data.clone();
            whole.timer.normalize_remaining(now);
            serde_json::to_vec(&whole).unwrap().len()
        };

        let start = Instant::now();
        for _ in 0..20 {
            serde_json::to_vec(&data).unwrap();
        }
        let per_save = start.elapsed() / 20;

        println!(
            "snapshot per state change: {per_change:?}, {bytes} bytes on the wire \
             (the whole store would be {everything} bytes; sessions are windowed)"
        );
        println!("compact JSON for save: {per_save:?} (before fsync)");
        println!(
            "steady-state IPC while the timer runs: 0 bytes/min \
             (was {} bytes/min when the window polled every second)",
            everything as u64 * 60
        );
    }
}
