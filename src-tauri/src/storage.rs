use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::domain::AppData;

/// The store holds captured notification text: message contents, sender names
/// and one-time codes. On a shared machine the default umask would leave it
/// world-readable, so the directory is owner-only (`rwx------`) and the file is
/// owner read/write (`rw-------`). Both are applied on every save, which repairs
/// a store written by an earlier build that did not set them.
#[cfg(unix)]
const DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

/// Applies `mode` to an existing path. A failure is not fatal — the data is
/// still written — but it is reported so a user on an exotic filesystem knows
/// the private contents are not protected.
#[cfg(unix)]
fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        eprintln!(
            "could not restrict permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) {}

#[cfg(not(unix))]
const DIRECTORY_MODE: u32 = 0;
#[cfg(not(unix))]
const FILE_MODE: u32 = 0;

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir.as_ref().join("pomodoro.json"),
        }
    }

    pub fn load(&self) -> Result<AppData, String> {
        if !self.path.exists() {
            return Ok(AppData::default());
        }

        let bytes = fs::read(&self.path)
            .map_err(|error| format!("could not read {}: {error}", self.path.display()))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("could not parse {}: {error}", self.path.display()))
    }

    pub fn save(&self, data: &AppData) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "the application data path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        restrict(parent, DIRECTORY_MODE);

        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| format!("could not serialize local data: {error}"))?;
        let mut file = fs::File::create(&temporary)
            .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
        // Narrowed before any bytes are written, so the private contents are
        // never briefly readable by other users on the machine.
        restrict(&temporary, FILE_MODE);
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &self.path).map_err(|error| {
            format!(
                "could not replace {} with saved data: {error}",
                self.path.display()
            )
        })?;
        restrict(&self.path, FILE_MODE);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_store_loads_defaults_and_round_trips() {
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-store-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        let store = Store::new(&directory);
        let mut data = store.load().expect("missing file should use defaults");
        data.create_task("Round trip", 2, 100);
        store.save(&data).expect("save should succeed");

        let loaded = store.load().expect("saved data should load");
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].title, "Round trip");

        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn saved_data_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "pomodoro-perm-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        let store = Store::new(&directory);
        store
            .save(&AppData::default())
            .expect("save should succeed");

        let mode = |path: &Path| {
            fs::metadata(path)
                .expect("path should exist")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&directory), DIRECTORY_MODE);
        assert_eq!(mode(&store.path), FILE_MODE);

        // A second save over an existing store keeps the narrow permissions.
        store
            .save(&AppData::default())
            .expect("resave should succeed");
        assert_eq!(mode(&store.path), FILE_MODE);

        let _ = fs::remove_dir_all(directory);
    }
}

/// The pull request's second manual check: carrying a real Kipindi store across
/// to the Pomodoro bundle id, as the README describes.
///
/// `#[ignore]`d because it reads the machine's actual
/// `~/.local/share/app.kipindi.timer/kipindi.json`, which CI does not have. It
/// never writes to either real directory — the copy goes to a temporary one.
/// Run it with `cargo test -- --ignored --nocapture kipindi`.
#[cfg(test)]
mod kipindi_migration {
    use super::*;

    #[test]
    #[ignore = "requires a real Kipindi store in the user's data directory"]
    fn a_real_kipindi_store_survives_the_move_to_pomodoro() {
        let home = std::env::var("HOME").expect("HOME must be set");
        let old = PathBuf::from(&home).join(".local/share/app.kipindi.timer/kipindi.json");
        assert!(
            old.exists(),
            "no Kipindi store at {} — nothing to migrate",
            old.display()
        );
        let before: serde_json::Value =
            serde_json::from_slice(&fs::read(&old).expect("the old store must be readable"))
                .expect("the old store must be valid JSON");

        // The README's `cp`, into a directory of our own so the real Pomodoro
        // store is never touched.
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-migration-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&directory).expect("the destination must be creatable");
        let store = Store::new(&directory);
        fs::copy(&old, &store.path).expect("the copy must succeed");

        let data = store.load().expect("the migrated store must load");

        // Tasks, settings and history all came across intact.
        let tasks = before["tasks"].as_array().expect("tasks array");
        assert_eq!(data.tasks.len(), tasks.len(), "every task survived");
        for (loaded, original) in data.tasks.iter().zip(tasks) {
            assert_eq!(loaded.id, original["id"].as_str().unwrap());
            assert_eq!(loaded.title, original["title"].as_str().unwrap());
            assert_eq!(
                u64::from(loaded.completed_pomodoros),
                original["completedPomodoros"].as_u64().unwrap()
            );
        }
        let sessions = before["sessions"].as_array().expect("sessions array");
        assert_eq!(
            data.sessions.len(),
            sessions.len(),
            "every session survived"
        );
        for (loaded, original) in data.sessions.iter().zip(sessions) {
            assert_eq!(loaded.id, original["id"].as_str().unwrap());
            assert_eq!(loaded.started_at, original["startedAt"].as_i64().unwrap());
        }
        assert_eq!(
            u64::from(data.settings.focus_minutes),
            before["settings"]["focusMinutes"].as_u64().unwrap()
        );
        assert_eq!(
            data.timer.active_task_id.as_deref(),
            before["timer"]["activeTaskId"].as_str()
        );

        // Fields the Kipindi build never wrote take their defaults, and capture
        // stays off rather than switching itself on during an upgrade.
        assert!(
            before["notifications"].is_null(),
            "precondition: an old store"
        );
        assert!(data.notifications.is_empty());
        assert!(!data.settings.notification_filter.enabled);
        assert!(!data.settings.silence_banners_during_focus);
        assert_eq!(data.banner_restore, None);

        // The copied file arrives with the shell's umask; the first save is what
        // tightens it.
        store.save(&data).expect("the migrated store must save");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&store.path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, FILE_MODE, "the migrated store is tightened on save");
        }
        let reloaded = store.load().expect("the saved store must load");
        assert_eq!(reloaded.tasks, data.tasks);
        assert_eq!(reloaded.sessions, data.sessions);

        let _ = fs::remove_dir_all(directory);
    }
}
