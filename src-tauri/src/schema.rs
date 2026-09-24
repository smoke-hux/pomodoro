//! The disk format has its own version; UI snapshots remain domain objects.
use crate::domain::{
    AppData, CaptureStatus, TimerStatus, NOTIFICATION_RETENTION, SESSION_RETENTION,
};
use serde_json::Value;
use std::{collections::HashSet, fmt};

pub const CURRENT_VERSION: u64 = 1;
pub const MAX_IMPORT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug)]
pub enum DecodeError {
    FutureVersion(u64),
    Invalid(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FutureVersion(version) => write!(f, "This data uses schema {version}; this build supports up to {CURRENT_VERSION}. Open it with a newer Pomodoro build."),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

fn document(bytes: &[u8]) -> Result<(Value, bool), DecodeError> {
    let mut value: Value = serde_json::from_slice(bytes)
        .map_err(|_| DecodeError::Invalid("The file is not valid Pomodoro JSON.".into()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| DecodeError::Invalid("The data must be a JSON object.".into()))?;
    let version = match object.get("schemaVersion") {
        None => 0,
        Some(value) => value.as_u64().ok_or_else(|| {
            DecodeError::Invalid("The data schema version must be a whole number.".into())
        })?,
    };
    if version > CURRENT_VERSION {
        return Err(DecodeError::FutureVersion(version));
    }
    // v0 -> v1: the original field layout is retained. Missing legacy fields
    // still receive their domain defaults; later migrations belong here.
    object.insert("schemaVersion".into(), CURRENT_VERSION.into());
    Ok((value, version < CURRENT_VERSION))
}

pub fn decode(bytes: &[u8]) -> Result<(AppData, bool), DecodeError> {
    let (value, migrated) = document(bytes)?;
    let data = serde_json::from_value(value).map_err(|_| {
        DecodeError::Invalid("The file contains invalid Pomodoro data fields.".into())
    })?;
    Ok((data, migrated))
}

pub fn encode(data: &AppData) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(data).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("Could not serialize Pomodoro data.")?;
    object.insert("schemaVersion".into(), CURRENT_VERSION.into());
    object.remove("captureStatus");
    object.remove("recoveredStore");
    serde_json::to_vec(&value).map_err(|error| error.to_string())
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, label: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || !seen.insert(id) {
            return Err(format!("The {label} contain an empty or duplicate ID."));
        }
    }
    Ok(())
}

/// Imports are stricter than legacy startup: an unrelated JSON object must
/// never silently become an empty replacement for a person's work.
pub fn import(bytes: &[u8]) -> Result<AppData, String> {
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err("Choose a data file smaller than 32 MiB.".into());
    }
    let (value, _) = document(bytes).map_err(|error| error.to_string())?;
    for name in ["tasks", "sessions", "interruptions"] {
        if !value.get(name).is_some_and(Value::is_array) {
            return Err(format!("The file is missing the {name} list."));
        }
    }
    for name in ["settings", "timer"] {
        if !value.get(name).is_some_and(Value::is_object) {
            return Err(format!("The file is missing its {name}."));
        }
    }
    let mut data: AppData = serde_json::from_value(value)
        .map_err(|_| "The file contains invalid Pomodoro data fields.".to_string())?;
    if data.sessions.len() > SESSION_RETENTION {
        return Err(format!(
            "The file contains {} sessions; Pomodoro can keep at most {SESSION_RETENTION}.",
            data.sessions.len()
        ));
    }
    if data.notifications.len() > NOTIFICATION_RETENTION {
        return Err(format!("The file contains {} notifications; Pomodoro can keep at most {NOTIFICATION_RETENTION}.", data.notifications.len()));
    }
    unique_ids(data.tasks.iter().map(|item| item.id.as_str()), "tasks")?;
    unique_ids(
        data.sessions.iter().map(|item| item.id.as_str()),
        "sessions",
    )?;
    unique_ids(
        data.interruptions.iter().map(|item| item.id.as_str()),
        "interruptions",
    )?;
    unique_ids(
        data.notifications.iter().map(|item| item.id.as_str()),
        "notifications",
    )?;
    if data.tasks.iter().any(|task| task.title.trim().is_empty()) {
        return Err("Every imported task needs a name.".into());
    }
    if data
        .sessions
        .iter()
        .any(|session| session.ended_at < session.started_at)
    {
        return Err("An imported session ends before it starts.".into());
    }
    if data.settings.clone().sanitized() != data.settings {
        return Err("Some imported settings are outside their supported limits.".into());
    }
    if data.timer.duration_seconds == 0
        || data.timer.duration_seconds > 24 * 60 * 60
        || data.timer.remaining_seconds > data.timer.duration_seconds
    {
        return Err("The imported timer has an invalid duration.".into());
    }
    let tasks: HashSet<_> = data.tasks.iter().map(|task| task.id.clone()).collect();
    if data
        .timer
        .active_task_id
        .as_ref()
        .is_some_and(|id| !tasks.contains(id))
    {
        data.timer.active_task_id = None;
    }
    for note in &mut data.interruptions {
        if note
            .converted_task_id
            .as_ref()
            .is_some_and(|id| !tasks.contains(id))
        {
            note.converted_task_id = None;
        }
    }
    for note in &mut data.notifications {
        if note.task_id.as_ref().is_some_and(|id| !tasks.contains(id)) {
            note.task_id = None;
        }
    }
    // Import never runs a timer or changes a desktop setting on another machine.
    if data.timer.status == TimerStatus::Running {
        data.timer.status = TimerStatus::Paused;
    }
    data.timer.ends_at = None;
    data.banner_restore = None;
    data.settings.notification_filter.enabled = false;
    data.settings.silence_banners_during_focus = false;
    data.capture_status = CaptureStatus::off();
    data.recovered_store = None;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_data_migrates_without_changing_tasks_or_settings() {
        let mut data = AppData::default();
        data.create_task("Keep my work", 3, 100);
        let (loaded, migrated) = decode(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert!(migrated);
        assert_eq!(loaded, data);
        let (again, migrated) = decode(&encode(&loaded).unwrap()).unwrap();
        assert!(!migrated);
        assert_eq!(again, data);
    }

    #[test]
    fn future_versions_are_never_read_as_current_data() {
        assert!(matches!(
            decode(br#"{"schemaVersion":99}"#),
            Err(DecodeError::FutureVersion(99))
        ));
        assert!(import(br#"{"schemaVersion":99}"#)
            .unwrap_err()
            .contains("newer"));
    }

    #[test]
    fn unrelated_and_inconsistent_imports_are_rejected() {
        assert!(import(b"{}").is_err());
        assert!(import(b"[]").is_err());
        assert!(import(b"not json").is_err());
        let mut data = AppData::default();
        data.create_task("A task", 1, 100);
        data.tasks.push(data.tasks[0].clone());
        assert!(import(&encode(&data).unwrap())
            .unwrap_err()
            .contains("duplicate"));
    }

    #[test]
    fn transfer_preserves_history_but_does_not_activate_desktop_features() {
        let mut data = AppData::default();
        data.create_task("A task", 1, 100);
        data.start_or_resume(100);
        data.settings.notification_filter.enabled = true;
        data.settings.silence_banners_during_focus = true;
        data.banner_restore = Some(true);
        let restored = import(&encode(&data).unwrap()).unwrap();
        assert_eq!(restored.tasks, data.tasks);
        assert_eq!(restored.timer.status, TimerStatus::Paused);
        assert_eq!(restored.timer.ends_at, None);
        assert!(!restored.settings.notification_filter.enabled);
        assert!(!restored.settings.silence_banners_during_focus);
        assert_eq!(restored.banner_restore, None);
    }

    #[test]
    fn imports_cannot_exceed_the_history_and_notification_retention_limits() {
        use crate::domain::{DesktopNotification, SessionRecord};

        let mut data = AppData {
            sessions: (0..SESSION_RETENTION)
                .map(|index| SessionRecord {
                    id: format!("session-{index}"),
                    ..SessionRecord::default()
                })
                .collect(),
            notifications: (0..NOTIFICATION_RETENTION)
                .map(|index| DesktopNotification {
                    id: format!("notification-{index}"),
                    ..DesktopNotification::default()
                })
                .collect(),
            ..AppData::default()
        };
        let restored = import(&encode(&data).unwrap()).unwrap();
        assert_eq!(restored.sessions.len(), SESSION_RETENTION);
        assert_eq!(restored.notifications.len(), NOTIFICATION_RETENTION);

        data.sessions.push(SessionRecord {
            id: "one-too-many".into(),
            ..SessionRecord::default()
        });
        assert!(import(&encode(&data).unwrap())
            .unwrap_err()
            .contains("sessions"));
        data.sessions.pop();
        data.notifications.push(DesktopNotification {
            id: "one-too-many".into(),
            ..DesktopNotification::default()
        });
        assert!(import(&encode(&data).unwrap())
            .unwrap_err()
            .contains("notifications"));
    }
}
