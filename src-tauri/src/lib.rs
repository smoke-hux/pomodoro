mod domain;
mod notifications;
mod storage;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use chrono::Utc;
use domain::{AppData, InterruptionCategory, NotificationFilter, Phase, Settings, TimerStatus};
use notifications::NotificationListener;
use storage::Store;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_notification::NotificationExt;

struct RuntimeState {
    data: Mutex<AppData>,
    store: Store,
    listener: Arc<NotificationListener>,
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

fn publish(app: &AppHandle, snapshot: &AppData) {
    let _ = app.emit("state-changed", snapshot.clone());
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
        .capture_notification(
            event.app_name,
            event.summary,
            event.body,
            event.urgency,
            now,
        )
        .is_none()
    {
        return;
    }
    if let Err(error) = state.store.save(&data) {
        eprintln!("could not save captured notification: {error}");
    }
    let snapshot = data.snapshot(now);
    drop(data);
    publish(app, &snapshot);
}

/// Brings the monitoring thread in line with the persisted filter. Idempotent,
/// so it is safe to call after any settings change.
fn sync_listener(app: &AppHandle, state: &RuntimeState, enabled: bool) {
    if !enabled {
        state.listener.stop();
        return;
    }
    let handle = app.clone();
    state
        .listener
        .start(move |event| record_notification(&handle, event));
}

fn reconcile(state: &RuntimeState, app: &AppHandle) -> Result<AppData, String> {
    let now = now_ms();
    let mut data = lock_data(state)?;
    let completed = data.tick(now);
    if completed.is_some() {
        state.store.save(&data)?;
    }
    let snapshot = data.snapshot(now);
    drop(data);

    if let Some(phase) = completed {
        publish(app, &snapshot);
        notify_boundary(app, phase, &snapshot);
    }
    Ok(snapshot)
}

fn mutate<F>(state: &RuntimeState, app: &AppHandle, action: F) -> Result<AppData, String>
where
    F: FnOnce(&mut AppData, i64) -> Result<(), String>,
{
    let now = now_ms();
    let mut data = lock_data(state)?;
    let completed = data.tick(now);
    action(&mut data, now)?;
    state.store.save(&data)?;
    let snapshot = data.snapshot(now);
    drop(data);

    publish(app, &snapshot);
    if let Some(phase) = completed {
        notify_boundary(app, phase, &snapshot);
    }
    Ok(snapshot)
}

fn toggle_timer_impl(state: &RuntimeState, app: &AppHandle) -> Result<AppData, String> {
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

#[tauri::command]
fn get_snapshot(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    reconcile(&state, &app)
}

#[tauri::command]
fn toggle_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    toggle_timer_impl(&state, &app)
}

#[tauri::command]
fn reset_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    mutate(&state, &app, |data, now| {
        data.reset(now);
        Ok(())
    })
}

#[tauri::command]
fn skip_phase(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    mutate(&state, &app, |data, now| {
        data.skip(now);
        Ok(())
    })
}

#[tauri::command]
fn set_phase(
    phase: Phase,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
fn toggle_task(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
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
fn delete_task(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
    mutate(&state, &app, move |data, now| {
        let text = data
            .interruptions
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.text.clone())
            .ok_or_else(|| "Interruption not found.".to_string())?;
        data.create_task(text, 1, now)
            .ok_or_else(|| "Could not create a task from this note.".to_string())?;
        data.handle_interruption(&id);
        Ok(())
    })
}

#[tauri::command]
fn update_settings(
    settings: Settings,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
    let snapshot = mutate(&state, &app, move |data, _| {
        data.update_settings(settings);
        Ok(())
    })?;
    sync_listener(&app, &state, snapshot.settings.notification_filter.enabled);
    Ok(snapshot)
}

#[tauri::command]
fn set_notification_filter(
    filter: NotificationFilter,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
    let snapshot = mutate(&state, &app, move |data, _| {
        data.settings.notification_filter = filter.sanitized();
        Ok(())
    })?;
    sync_listener(&app, &state, snapshot.settings.notification_filter.enabled);
    Ok(snapshot)
}

#[tauri::command]
fn triage_notification(
    id: String,
    triaged: bool,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
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
) -> Result<AppData, String> {
    mutate(&state, &app, move |data, now| {
        let summary = data
            .notifications
            .iter()
            .find(|notification| notification.id == id)
            .map(|notification| notification.summary.clone())
            .ok_or_else(|| "Notification not found.".to_string())?;
        data.create_task(summary, 1, now)
            .ok_or_else(|| "This notification has no summary to name a task.".to_string())?;
        data.set_notification_triaged(&id, true);
        Ok(())
    })
}

#[tauri::command]
fn delete_notification(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
    mutate(&state, &app, move |data, _| {
        if !data.delete_notification(&id) {
            return Err("Notification not found.".to_string());
        }
        Ok(())
    })
}

#[tauri::command]
fn clear_notifications(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    mutate(&state, &app, |data, _| {
        data.clear_notifications();
        Ok(())
    })
}

#[tauri::command]
fn clear_history(state: State<'_, RuntimeState>, app: AppHandle) -> Result<AppData, String> {
    mutate(&state, &app, |data, _| {
        data.sessions.clear();
        Ok(())
    })
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
    let toggle = MenuItem::with_id(app, "toggle", "Start / Pause", true, None::<&str>)?;
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
            "quit" => app.exit(0),
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
            });
            build_tray(app)?;

            let handle = app.handle().clone();
            sync_listener(&handle, &handle.state::<RuntimeState>(), capture_enabled);

            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(500));
                let state = handle.state::<RuntimeState>();
                if let Err(error) = reconcile(&state, &handle) {
                    eprintln!("timer reconciliation failed: {error}");
                }
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
            set_notification_filter,
            triage_notification,
            convert_notification,
            delete_notification,
            clear_notifications,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pomodoro");
}
