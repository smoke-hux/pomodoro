mod domain;
mod notifications;
mod quiet;
mod storage;

use std::{
    sync::{
        atomic::{AtomicI64, AtomicI8, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

use chrono::Utc;
use domain::{AppData, CaptureStatus, InterruptionCategory, Phase, Settings, TimerStatus};
use notifications::NotificationListener;
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

struct RuntimeState {
    data: Mutex<AppData>,
    store: Store,
    listener: Arc<NotificationListener>,
    /// When the oldest unsaved captured notification arrived, or 0 for none.
    notifications_dirty_since: AtomicI64,
    /// The last banner state [`sync_quiet`] acted on: `1` quiet, `0` normal,
    /// `-1` not yet decided. Asking the desktop what its banner setting is
    /// means spawning a process; this makes that happen once per transition
    /// rather than once per wake-up.
    quiet_desire: AtomicI8,
    /// What the reconciliation thread sleeps on. Anything that changes the
    /// timer or files a notification calls [`Self::nudge`] so the thread
    /// re-evaluates immediately instead of at its next scheduled wake-up.
    wake: Condvar,
    wake_lock: Mutex<()>,
    /// The tray's start/pause item and the label it currently shows, set once
    /// the tray is built. Kept so the item can say what it will do.
    tray_toggle: OnceLock<(MenuItem<tauri::Wry>, Mutex<&'static str>)>,
}

/// While a phase is counting down the thread wakes at least this often, so a
/// completion is never late by more than this even if the monotonic clock
/// stood still across a suspend.
const RUNNING_WAKE: Duration = Duration::from_secs(1);
/// With nothing counting down and nothing waiting to be written there is
/// nothing to do. A long ceiling rather than no ceiling, as a backstop.
const IDLE_WAKE: Duration = Duration::from_secs(30);

impl RuntimeState {
    /// Wakes the reconciliation thread.
    fn nudge(&self) {
        self.wake.notify_all();
    }

    /// How long the reconciliation thread can sleep before something needs it.
    ///
    /// Running: until the deadline, capped at [`RUNNING_WAKE`]. Idle with a
    /// pending notification write: until that is due. Otherwise: a long time.
    /// The old loop woke twice a second, all day, whether or not anything was
    /// counting down.
    fn next_wait(&self, now_ms: i64) -> Duration {
        let running_deadline = self
            .data
            .lock()
            .ok()
            .filter(|data| data.timer.status == TimerStatus::Running)
            .and_then(|data| data.timer.ends_at);
        if let Some(ends_at) = running_deadline {
            let until = u64::try_from(ends_at.saturating_sub(now_ms)).unwrap_or(0);
            return Duration::from_millis(until).min(RUNNING_WAKE);
        }

        let dirty_since = self.notifications_dirty_since.load(Ordering::Acquire);
        if dirty_since != 0 {
            let due = dirty_since.saturating_add(NOTIFICATION_FLUSH_MS);
            let until = u64::try_from(due.saturating_sub(now_ms)).unwrap_or(0);
            return Duration::from_millis(until).min(IDLE_WAKE);
        }

        IDLE_WAKE
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

    /// Writes captured notifications once they have been waiting long enough.
    /// Called from the reconciliation loop, so a quiet burst still lands within
    /// a couple of seconds.
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
        if let Err(error) = self.store.save(&data) {
            eprintln!("could not save captured notifications: {error}");
            return;
        }
        self.clear_notifications_dirty();
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

/// What the tray's start/pause item will do if clicked now. It used to read
/// "Start / Pause" whatever the timer was doing, and the tray is exactly where
/// the window is not there to show which one applies.
fn toggle_label(status: TimerStatus, phase: Phase) -> &'static str {
    match (status, phase) {
        (TimerStatus::Running, _) => "Pause",
        (TimerStatus::Paused, _) => "Resume",
        (TimerStatus::Idle, Phase::Focus) => "Start focus",
        (TimerStatus::Idle, Phase::ShortBreak | Phase::LongBreak) => "Start break",
    }
}

/// Relabels the tray item, and only when the label actually changes: most
/// publishes are a task edit or a filed notification, not a timer transition.
fn sync_tray(app: &AppHandle, snapshot: &AppData) {
    let state = app.state::<RuntimeState>();
    let Some((item, shown)) = state.tray_toggle.get() else {
        return;
    };
    let label = toggle_label(snapshot.timer.status, snapshot.timer.phase);
    // Released before the item is touched: relabelling hops to the main
    // thread and waits for it, and the main thread comes through here too
    // when the tray menu itself is clicked.
    let changed = shown
        .lock()
        .map(|mut shown| std::mem::replace(&mut *shown, label) != label)
        .unwrap_or(false);
    if changed {
        let _ = item.set_text(label);
    }
}

/// Broadcasts the new state to the window. This is the one channel through
/// which the UI learns anything: commands do not return the state as well, so
/// each change is serialised and sent once, not twice.
fn publish(app: &AppHandle, snapshot: &AppData) {
    let _ = app.emit("state-changed", snapshot);
    sync_tray(app, snapshot);
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
    let snapshot = data.snapshot(now);
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
    let snapshot = data.snapshot(now_ms());
    drop(data);
    publish(app, &snapshot);
}

/// Brings the desktop's banner setting in line with the current phase, and
/// reports whether the persisted restore marker moved.
///
/// The marker holds the value to put back, so an app that is killed mid-focus
/// leaves behind everything the next launch needs to repair the desktop.
fn sync_quiet(state: &RuntimeState, data: &mut AppData, now_ms: i64) -> bool {
    let want_quiet = data.settings.silence_banners_during_focus && data.is_focus_running(now_ms);
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

/// Moves the timer forward to `now`, and only if a phase completed persists,
/// broadcasts and alerts. Nothing is cloned or serialised on the common path
/// where nothing happened, which is nearly every wake-up.
fn advance(state: &RuntimeState, app: &AppHandle) -> Result<(), String> {
    let now = now_ms();
    let mut data = lock_data(state)?;
    let completed = data.tick(now);
    let quiet_changed = sync_quiet(state, &mut data, now);
    if completed.is_none() && !quiet_changed {
        return Ok(());
    }
    state.store.save(&data)?;
    state.clear_notifications_dirty();
    let snapshot = data.snapshot(now);
    drop(data);

    publish(app, &snapshot);
    if let Some(phase) = completed {
        notify_boundary(app, phase, &snapshot);
    }
    Ok(())
}

fn mutate<F>(state: &RuntimeState, app: &AppHandle, action: F) -> Result<(), String>
where
    F: FnOnce(&mut AppData, i64) -> Result<(), String>,
{
    let now = now_ms();
    let mut data = lock_data(state)?;
    let completed = data.tick(now);
    action(&mut data, now)?;
    sync_quiet(state, &mut data, now);
    state.store.save(&data)?;
    // The write above covers everything, captured notifications included.
    state.clear_notifications_dirty();
    let snapshot = data.snapshot(now);
    drop(data);

    // The timer may have started, stopped or moved; the sleeping thread needs
    // to recompute how long it can wait.
    state.nudge();
    publish(app, &snapshot);
    if let Some(phase) = completed {
        notify_boundary(app, phase, &snapshot);
    }
    Ok(())
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
    advance(&state, &app)?;
    let data = lock_data(&state)?;
    Ok(data.snapshot(now_ms()))
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

#[tauri::command]
fn toggle_task(id: String, state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, move |data, now| {
        let done = data
            .tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.done)
            .ok_or_else(|| "Task not found.".to_string())?;
        if !data.set_task_done(&id, !done, now) {
            return Err("Task not found.".to_string());
        }
        Ok(())
    })
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
    mutate(&state, &app, move |data, _| {
        data.update_settings(settings);
        Ok(())
    })?;
    sync_listener(&app, &state, enabled);
    Ok(())
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

/// Leaves the machine as Pomodoro found it: banners back on if they were turned
/// off for a focus interval, and captured notifications written rather than
/// waiting on the debounce.
fn shut_down(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    state.listener.stop();
    state.quiet_desire.store(0, Ordering::Release);
    if let Ok(mut data) = state.data.lock() {
        if let Some(previous) = data.banner_restore.take() {
            if let Err(error) = quiet::write_show_banners(previous) {
                eprintln!("could not restore notification banners: {error}");
            }
        }
        if let Err(error) = state.store.save(&data) {
            eprintln!("could not save on shutdown: {error}");
        }
    }
    state.clear_notifications_dirty();
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
    let label = state
        .data
        .lock()
        .map(|data| toggle_label(data.timer.status, data.timer.phase))
        .unwrap_or("Start / Pause");
    let toggle = MenuItem::with_id(app, "toggle", label, true, None::<&str>)?;
    let _ = state.tray_toggle.set((toggle.clone(), Mutex::new(label)));
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
            let data = match store.load() {
                Ok(data) => data,
                Err(error) => {
                    eprintln!("{error}; starting with an empty local data set");
                    AppData::default()
                }
            };
            let capture_enabled = data.settings.notification_filter.enabled;
            app.manage(RuntimeState {
                data: Mutex::new(data),
                store,
                listener: Arc::new(NotificationListener::new()),
                notifications_dirty_since: AtomicI64::new(0),
                quiet_desire: AtomicI8::new(-1),
                wake: Condvar::new(),
                wake_lock: Mutex::new(()),
                tray_toggle: OnceLock::new(),
            });
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
            thread::spawn(move || loop {
                let state = handle.state::<RuntimeState>();
                let wait = state.next_wait(now_ms());
                if let Ok(guard) = state.wake_lock.lock() {
                    let _ = state.wake.wait_timeout(guard, wait);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pomodoro");
}

#[cfg(test)]
mod tray {
    use super::*;

    #[test]
    fn the_tray_item_names_what_a_click_will_do() {
        assert_eq!(toggle_label(TimerStatus::Running, Phase::Focus), "Pause");
        assert_eq!(toggle_label(TimerStatus::Running, Phase::ShortBreak), "Pause");
        assert_eq!(toggle_label(TimerStatus::Paused, Phase::Focus), "Resume");
        assert_eq!(toggle_label(TimerStatus::Idle, Phase::Focus), "Start focus");
        assert_eq!(toggle_label(TimerStatus::Idle, Phase::ShortBreak), "Start break");
        assert_eq!(toggle_label(TimerStatus::Idle, Phase::LongBreak), "Start break");
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
