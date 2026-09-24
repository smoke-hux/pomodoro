//! Native file selection stays in Rust. The webview confirms a validated
//! preview token; it cannot request arbitrary filesystem reads or writes.
use crate::{
    domain::{AppData, TimerStatus},
    runtime::{lock_data, now_ms, publish, sync_listener, RuntimeState},
    schema,
    storage::{write_private, Store},
};
use serde::Serialize;
use std::{fs, io::Read, path::Path, sync::Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

struct PreparedImport {
    token: String,
    data: AppData,
}

#[derive(Default)]
pub(crate) struct TransferState(Mutex<Option<PreparedImport>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportPreview {
    token: String,
    file_name: String,
    tasks: usize,
    sessions: usize,
    interruptions: usize,
    notifications: usize,
}

fn allowed_export(path: &Path, store: &Store) -> Result<(), String> {
    let parent = path.parent().ok_or("Choose a folder for the export.")?;
    let parent = parent.canonicalize().map_err(|error| error.to_string())?;
    if let Ok(data_directory) = store.directory().canonicalize() {
        if parent.starts_with(data_directory) {
            return Err("Choose a folder outside Pomodoro's managed data folder.".into());
        }
    }
    Ok(())
}

fn export_bytes(data: &AppData) -> Result<Vec<u8>, String> {
    let bytes = schema::encode(data)?;
    if bytes.len() as u64 > schema::MAX_IMPORT_BYTES {
        return Err("The full backup exceeds the 32 MiB import limit. Open the data folder to copy it manually.".into());
    }
    Ok(bytes)
}

#[tauri::command(async)]
pub(crate) fn export_data(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Export Pomodoro data")
        .add_filter("Pomodoro data", &["json"])
        .set_file_name("pomodoro-backup.json")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|error| error.to_string())?;
    allowed_export(&path, &state.store)?;
    let mut data = lock_data(&state)?.clone();
    data.pause_to_quit(now_ms());
    data.banner_restore = None;
    write_private(&path, &export_bytes(&data)?)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command(async)]
pub(crate) fn preview_import(
    app: AppHandle,
    transfer: State<'_, TransferState>,
) -> Result<Option<ImportPreview>, String> {
    *transfer
        .0
        .lock()
        .map_err(|_| "The import preview is unavailable.")? = None;
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Choose Pomodoro data to import")
        .add_filter("Pomodoro data", &["json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|error| error.to_string())?;
    let file = fs::File::open(&path)
        .map_err(|error| format!("Could not read the chosen file: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Choose a regular JSON data file.".into());
    }
    let mut bytes = Vec::new();
    file.take(schema::MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the chosen file: {error}"))?;
    let data = schema::import(&bytes)?;
    let token = uuid::Uuid::new_v4().to_string();
    let preview = ImportPreview {
        token: token.clone(),
        file_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        tasks: data.tasks.len(),
        sessions: data.sessions.len(),
        interruptions: data.interruptions.len(),
        notifications: data.notifications.len(),
    };
    *transfer
        .0
        .lock()
        .map_err(|_| "The import preview is unavailable.")? = Some(PreparedImport { token, data });
    Ok(Some(preview))
}

/// Persist both recovery point and replacement before changing in-memory data.
fn replace(
    store: &Store,
    current: &mut AppData,
    imported: &AppData,
    now: i64,
) -> Result<(), String> {
    store.ensure_writable()?;
    if current.timer.status != TimerStatus::Idle {
        return Err("Reset the current interval before replacing your data.".into());
    }
    let mut next = imported.clone();
    next.banner_restore = current.banner_restore;
    store.backup(current, now)?;
    store.save(&next)?;
    *current = next;
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn confirm_import(
    token: String,
    app: AppHandle,
    state: State<'_, RuntimeState>,
    transfer: State<'_, TransferState>,
) -> Result<(), String> {
    let snapshot = {
        let mut pending = transfer
            .0
            .lock()
            .map_err(|_| "The import preview is unavailable.")?;
        let prepared = pending
            .as_ref()
            .filter(|item| item.token == token)
            .ok_or("This preview has expired. Choose the file again.")?;
        let mut current = lock_data(&state)?;
        let now = now_ms();
        replace(&state.store, &mut current, &prepared.data, now)?;
        *pending = None;
        state.snapshot(&current, now)
    };
    sync_listener(&app, &state, false);
    state.nudge();
    publish(&app, &snapshot);
    Ok(())
}

#[tauri::command]
pub(crate) fn cancel_import(
    token: String,
    transfer: State<'_, TransferState>,
) -> Result<(), String> {
    let mut pending = transfer
        .0
        .lock()
        .map_err(|_| "The import preview is unavailable.")?;
    // An old dialog's asynchronous cleanup must not cancel a newer preview.
    if pending
        .as_ref()
        .is_some_and(|prepared| prepared.token == token)
    {
        *pending = None;
    }
    Ok(())
}

fn csv_cell(text: &str) -> String {
    // Quoting alone does not prevent spreadsheet programs treating user text
    // as a formula. An apostrophe keeps those cells as text.
    let prefix = if text.trim_start().starts_with(['=', '+', '-', '@'])
        || text.starts_with(['\t', '\r', '\n'])
    {
        "'"
    } else {
        ""
    };
    format!("\"{prefix}{}\"", text.replace('"', "\"\""))
}

fn sessions_csv(data: &AppData) -> String {
    let mut csv = String::from(
        "id,phase,task_id,task_title,duration_seconds,started_at_utc,ended_at_utc,outcome\r\n",
    );
    for session in &data.sessions {
        let timestamp = |value| {
            chrono::DateTime::from_timestamp_millis(value)
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| value.to_string())
        };
        let phase = serde_json::to_value(session.phase).unwrap_or_default();
        let outcome = serde_json::to_value(session.outcome).unwrap_or_default();
        let cells = [
            session.id.clone(),
            phase.as_str().unwrap_or_default().into(),
            session.task_id.clone().unwrap_or_default(),
            session.task_title.clone().unwrap_or_default(),
            session.duration_seconds.to_string(),
            timestamp(session.started_at),
            timestamp(session.ended_at),
            outcome.as_str().unwrap_or_default().into(),
        ];
        csv.push_str(
            &cells
                .iter()
                .map(|cell| csv_cell(cell))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
    }
    csv
}

#[tauri::command(async)]
pub(crate) fn export_sessions_csv(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Export session history")
        .add_filter("CSV spreadsheet", &["csv"])
        .set_file_name("pomodoro-sessions.csv")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|error| error.to_string())?;
    allowed_export(&path, &state.store)?;
    let data = lock_data(&state)?;
    let csv = sessions_csv(&data);
    drop(data);
    write_private(&path, csv.as_bytes())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command(async)]
pub(crate) fn open_data_location(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<(), String> {
    let directory = state.store.directory();
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|error| format!("Could not open the data folder: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_backs_up_old_data_and_commits_new_data() {
        let directory =
            std::env::temp_dir().join(format!("pomodoro-import-{}", uuid::Uuid::new_v4()));
        let store = Store::new(&directory);
        let mut current = AppData::default();
        current.create_task("Before", 1, 10);
        current.banner_restore = Some(true);
        store.save(&current).unwrap();
        let mut imported = AppData::default();
        imported.create_task("After", 2, 20);
        replace(&store, &mut current, &imported, 30).unwrap();
        assert_eq!(current.tasks[0].title, "After");
        assert_eq!(store.load().unwrap().tasks[0].title, "After");
        assert_eq!(current.banner_restore, Some(true));
        assert_eq!(store.load().unwrap().banner_restore, Some(true));
        let backup = fs::read_dir(directory.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            schema::decode(&fs::read(backup).unwrap()).unwrap().0.tasks[0].title,
            "Before"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn active_interval_and_backup_failure_leave_memory_unchanged() {
        let directory =
            std::env::temp_dir().join(format!("pomodoro-import-{}", uuid::Uuid::new_v4()));
        let store = Store::new(&directory);
        let mut current = AppData::default();
        current.start_or_resume(10);
        let before = current.clone();
        assert!(replace(&store, &mut current, &AppData::default(), 20).is_err());
        assert_eq!(current, before);
        current = AppData::default();
        current.create_task("Keep", 1, 10);
        store.save(&current).unwrap();
        fs::write(directory.join("backups"), "blocks backup directory").unwrap();
        let before = current.clone();
        assert!(replace(&store, &mut current, &AppData::default(), 20).is_err());
        assert_eq!(current, before);
        assert_eq!(store.load().unwrap(), before);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_replacement_save_failure_preserves_memory_and_the_recovery_point() {
        let directory =
            std::env::temp_dir().join(format!("pomodoro-import-{}", uuid::Uuid::new_v4()));
        let store = Store::new(&directory);
        let mut current = AppData::default();
        current.create_task("Keep this work", 1, 10);
        let before = current.clone();
        // A directory at the destination makes the final atomic rename fail,
        // after the recovery point has already been written successfully.
        fs::create_dir_all(directory.join("pomodoro.json")).unwrap();

        assert!(replace(&store, &mut current, &AppData::default(), 20).is_err());
        assert_eq!(current, before);
        assert!(directory.join("pomodoro.json").is_dir());
        let backup = fs::read_dir(directory.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            schema::decode(&fs::read(backup).unwrap()).unwrap().0,
            before
        );
        assert_eq!(
            fs::read_dir(&directory).unwrap().count(),
            2,
            "no temporary replacement survives"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn csv_escapes_text_and_neutralizes_spreadsheet_formulas() {
        assert_eq!(csv_cell("A,\"B\"\nC"), "\"A,\"\"B\"\"\nC\"");
        assert_eq!(csv_cell("=1+1"), "\"'=1+1\"");
        assert_eq!(csv_cell("  @SUM(A1)"), "\"'  @SUM(A1)\"");
        let mut data = AppData::default();
        data.create_task("Report", 1, 0);
        data.start_or_resume(1_000);
        data.skip(2_000);
        let csv = sessions_csv(&data);
        assert!(csv.contains("1970-01-01T00:00:01+00:00"));
        assert!(csv.contains("skipped"));
    }

    #[test]
    fn full_exports_fit_the_limit_used_when_importing_them() {
        let mut data = AppData::default();
        data.create_task("Portable work", 1, 0);
        let exported = export_bytes(&data).unwrap();
        assert_eq!(schema::import(&exported).unwrap().tasks, data.tasks);

        data.tasks[0].title = "x".repeat(schema::MAX_IMPORT_BYTES as usize);
        assert!(export_bytes(&data).unwrap_err().contains("32 MiB"));
    }
}
