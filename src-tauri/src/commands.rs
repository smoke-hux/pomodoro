//! Tauri IPC commands. Mutations share the runtime's reconciliation and save path.

use crate::{
    alerts::{task_alert, task_complete_summary, TaskAlert},
    domain::{AppData, InterruptionCategory, Phase, Settings, TimerStatus},
    runtime::{advance, lock_data, mutate, now_ms, sync_listener, toggle_timer_impl, RuntimeState},
    sound::{self, Cue},
};
use tauri::{AppHandle, State};
use tauri_plugin_notification::NotificationExt;

/// The one command that returns state: the window asks once, on load, and
/// follows `state-changed` from then on.
#[tauri::command]
pub(crate) fn get_snapshot(
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<AppData, String> {
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
pub(crate) fn toggle_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    toggle_timer_impl(&state, &app)
}

#[tauri::command]
pub(crate) fn reset_timer(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        data.reset(now);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn skip_phase(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        data.skip(now);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn set_phase(
    phase: Phase,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        if data.timer.status != TimerStatus::Idle {
            return Err("Reset the current interval before changing timer mode.".to_string());
        }
        data.set_phase(phase, now);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn select_task(
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
pub(crate) fn add_task(
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
pub(crate) fn update_task(
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
pub(crate) fn toggle_task(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
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
pub(crate) fn preview_sound() -> Result<(), String> {
    sound::play_and_report(Cue::IntervalFinished)
}

#[tauri::command]
pub(crate) fn delete_task(
    id: String,
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
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
pub(crate) fn capture_interruption(
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
pub(crate) fn set_interruption_handled(
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
pub(crate) fn delete_interruption(
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
pub(crate) fn convert_interruption_to_task(
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
pub(crate) fn update_settings(
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
pub(crate) fn triage_notification(
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
pub(crate) fn convert_notification(
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
pub(crate) fn delete_notification(
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
pub(crate) fn clear_notifications(
    state: State<'_, RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    mutate(&state, &app, |data, _| {
        data.clear_notifications();
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn clear_history(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    mutate(&state, &app, |data, now| {
        state.store.backup(data, now)?;
        data.sessions.clear();
        Ok(())
    })
}
