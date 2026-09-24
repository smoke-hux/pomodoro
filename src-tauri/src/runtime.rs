//! State ownership, persistence, reconciliation, and desktop integration.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicI8, AtomicU8, Ordering},
        Arc, Condvar, Mutex, OnceLock, PoisonError,
    },
    thread,
    time::Duration,
};

use crate::{
    alerts::notify_boundary,
    domain::{AppData, CaptureStatus, Phase, TimerStatus},
    notifications::{self, NotificationListener},
    quiet,
    sound::{self, Cue},
    storage::Store,
    tray::{build_tray, sync_tray, toggle_label_index},
};
use chrono::Utc;
use tauri::{menu::MenuItem, AppHandle, Emitter, Manager};

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

pub(crate) struct RuntimeState {
    pub(crate) data: Mutex<AppData>,
    pub(crate) store: Store,
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
    pub(crate) tray_toggle: OnceLock<MenuItem<tauri::Wry>>,
    /// Indexes into [`crate::tray::TOGGLE_LABELS`]: what the item should say, recorded under
    /// the data lock, and what it says, touched only on the main thread.
    pub(crate) tray_wanted: AtomicU8,
    pub(crate) tray_shown: AtomicU8,
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
    pub(crate) fn nudge(&self) {
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
    pub(crate) fn save(&self, data: &AppData, now_ms: i64) -> Result<(), String> {
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

pub(crate) fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn lock_data(
    state: &RuntimeState,
) -> Result<std::sync::MutexGuard<'_, AppData>, String> {
    state
        .data
        .lock()
        .map_err(|_| "local timer state is unavailable".to_string())
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
    pub(crate) fn snapshot(&self, data: &AppData, now_ms: i64) -> AppData {
        self.tray_wanted.store(
            toggle_label_index(data.timer.status, data.timer.phase),
            Ordering::SeqCst,
        );
        data.snapshot(now_ms)
    }
}

/// Broadcasts the new state to the window. This is the one channel through
/// which the UI learns anything: commands do not return the state as well, so
/// each change is serialised and sent once, not twice.
pub(crate) fn publish(app: &AppHandle, snapshot: &AppData) {
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
pub(crate) fn sync_listener(app: &AppHandle, state: &RuntimeState, enabled: bool) {
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
pub(crate) fn advance(state: &RuntimeState, app: &AppHandle) -> Result<(), String> {
    broadcast(app, reconcile(state, now_ms()))
}

/// [`apply`], broadcast. What every command that changes anything goes through.
pub(crate) fn mutate<F>(state: &RuntimeState, app: &AppHandle, action: F) -> Result<(), String>
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

pub(crate) fn toggle_timer_impl(state: &RuntimeState, app: &AppHandle) -> Result<(), String> {
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
    ///   in fact already due, it is completed instead, but the next one is not
    ///   started: see [`AppData::pause_to_quit`].
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
            data.pause_to_quit(now_ms);
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

pub(crate) fn shut_down(app: &AppHandle) {
    app.state::<RuntimeState>().shut_down(now_ms());
}

/// Loads persisted state and starts the one reconciliation worker.
pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
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
}

#[cfg(test)]
mod live_gnome;
#[cfg(test)]
mod snapshot_cost;
#[cfg(test)]
mod tests;
