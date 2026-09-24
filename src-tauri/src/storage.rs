use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{domain::AppData, schema};

#[cfg(test)]
#[path = "storage_tests.rs"]
mod migration_tests;

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
    /// False once an unreadable store could be neither moved nor copied aside.
    /// The file at `path` is then the only copy of whatever the person had,
    /// and [`Store::save`] leaves it alone for the rest of the session.
    writable: AtomicBool,
    migration_pending: AtomicBool,
}

impl Store {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir.as_ref().join("pomodoro.json"),
            writable: AtomicBool::new(true),
            migration_pending: AtomicBool::new(false),
        }
    }

    pub fn load(&self) -> Result<AppData, String> {
        if !self.path.exists() {
            return Ok(AppData::default());
        }

        let bytes = fs::read(&self.path)
            .map_err(|error| format!("could not read {}: {error}", self.path.display()))?;
        match schema::decode(&bytes) {
            Ok((data, migrated)) => {
                self.migration_pending.store(migrated, Ordering::Release);
                Ok(data)
            }
            Err(error) => {
                // A downgrade must leave the newer store at its original path.
                if matches!(error, schema::DecodeError::FutureVersion(_)) {
                    self.writable.store(false, Ordering::Release);
                }
                Err(format!("could not read {}: {error}", self.path.display()))
            }
        }
    }

    /// Loads the store, and if it exists but cannot be read, moves it aside
    /// before anything can be saved over it.
    ///
    /// Starting from defaults after a failed load used to be silent, and the
    /// first save — any click at all — then replaced the unreadable file with
    /// those defaults. A file that will not parse today may still be
    /// recoverable by hand, so it is kept, under a name that says when it was
    /// set aside, and that name travels in `recovered_store` so the UI can
    /// show it.
    ///
    /// Setting aside can itself fail — a directory that cannot be written to
    /// refuses the rename and the copy alike. The app still ran after that,
    /// and the next save renamed a temporary file over the unreadable
    /// original: the very loss this function exists to prevent, one step
    /// later. So a file that could not be kept closes the store to writing,
    /// and `recovered_store` is left empty to say there is no copy.
    pub fn load_or_set_aside(&self, now_ms: i64) -> AppData {
        let error = match self.load() {
            Ok(data) => return data,
            Err(error) => error,
        };
        eprintln!("{error}; starting with an empty local data set");

        if !self.writable.load(Ordering::Acquire) {
            return AppData {
                recovered_store: Some(String::new()),
                ..AppData::default()
            };
        }

        let aside = self
            .path
            .with_file_name(format!("pomodoro.unreadable-{now_ms}.json"));
        // Copying is the fallback for a file that can be read but not moved.
        let kept = fs::rename(&self.path, &aside).is_ok() || fs::copy(&self.path, &aside).is_ok();
        let name = if kept {
            // It may hold captured notification text, like the store itself.
            restrict(&aside, FILE_MODE);
            aside
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            eprintln!(
                "could not set the unreadable store aside; nothing will be saved this session"
            );
            self.writable.store(false, Ordering::Release);
            String::new()
        };
        AppData {
            recovered_store: Some(name),
            ..AppData::default()
        }
    }

    /// Writes the data to disk, unless the store has been closed to writing.
    ///
    /// A closed store answers `Ok` without touching the disk at all — no
    /// directory created, no permissions changed, no temporary file — and the
    /// session runs in memory only. Refusing with an error instead would fail
    /// every command at its `store.save(&data)?` and freeze the UI over a file
    /// the person cannot do anything about from inside the app. Doing nothing
    /// lets them keep using the timer while the banner tells them that nothing
    /// is being saved.
    pub fn save(&self, data: &AppData) -> Result<(), String> {
        if !self.writable.load(Ordering::Acquire) {
            return Ok(());
        }

        let parent = self
            .path
            .parent()
            .ok_or_else(|| "the application data path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        restrict(parent, DIRECTORY_MODE);

        if self.migration_pending.load(Ordering::Acquire) && self.path.exists() {
            let previous = fs::read(&self.path).map_err(|error| error.to_string())?;
            self.backup_bytes(&previous, chrono::Utc::now().timestamp_millis())?;
        }
        write_private(&self.path, &schema::encode(data)?)?;
        self.migration_pending.store(false, Ordering::Release);
        Ok(())
    }

    pub fn directory(&self) -> &Path {
        self.path.parent().expect("the store always has a parent")
    }

    pub fn ensure_writable(&self) -> Result<(), String> {
        if self.writable.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("The local data file is protected. Repair it and restart Pomodoro before importing or clearing data.".into())
        }
    }

    /// Keep the five newest recovery points. Called before imports, schema
    /// migration and clearing session history, not when deleting private notes.
    pub fn backup(&self, data: &AppData, now_ms: i64) -> Result<PathBuf, String> {
        self.ensure_writable()?;
        self.backup_bytes(&schema::encode(data)?, now_ms)
    }

    fn backup_bytes(&self, bytes: &[u8], now_ms: i64) -> Result<PathBuf, String> {
        let directory = self.directory().join("backups");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        restrict(self.directory(), DIRECTORY_MODE);
        restrict(&directory, DIRECTORY_MODE);
        let path = directory.join(format!(
            "pomodoro-{now_ms:020}-{}.json",
            uuid::Uuid::new_v4()
        ));
        write_private(&path, bytes)?;
        let mut backups: Vec<_> = fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("pomodoro-")
                    && name.ends_with(".json")
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
            })
            .map(|entry| entry.path())
            // The new recovery point must survive even when timestamps tie
            // or the wall clock moved backwards. Keep it plus four older ones.
            .filter(|existing| existing != &path)
            .collect();
        backups.sort();
        let obsolete = backups.len().saturating_sub(4);
        for old in backups.into_iter().take(obsolete) {
            fs::remove_file(&old).map_err(|error| format!("Could not rotate a backup: {error}"))?;
        }
        Ok(path)
    }
}

/// Create the temporary file with private permissions from its first byte,
/// then replace atomically. Export failures cannot truncate an existing file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The chosen file has no parent folder.")?;
    let temporary = parent.join(format!(".pomodoro-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(FILE_MODE);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("Could not create the saved file: {error}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("Could not write the saved file: {error}"))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("Could not replace the saved file: {error}"))?;
        // Once rename succeeds the change is committed. A directory sync
        // failure must not make callers keep old in-memory data after an import.
        #[cfg(unix)]
        if let Err(error) = fs::File::open(parent).and_then(|directory| directory.sync_all()) {
            eprintln!("could not sync the data directory: {error}");
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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

    #[test]
    fn an_unreadable_store_is_set_aside_rather_than_saved_over() {
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-aside-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = Store::new(&directory);
        fs::write(directory.join("pomodoro.json"), b"{ \"tasks\": [ truncated").unwrap();

        let data = store.load_or_set_aside(1_234);
        assert!(data.tasks.is_empty());
        assert_eq!(
            data.recovered_store.as_deref(),
            Some("pomodoro.unreadable-1234.json")
        );

        // The first save after that is what used to destroy the old file.
        store.save(&data).expect("save should succeed");
        let kept = fs::read(directory.join("pomodoro.unreadable-1234.json")).unwrap();
        assert_eq!(kept, b"{ \"tasks\": [ truncated");

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_missing_or_readable_store_is_not_a_recovery() {
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-fresh-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        let store = Store::new(&directory);
        let data = store.load_or_set_aside(1);
        assert_eq!(data.recovered_store, None);

        store.save(&data).expect("save should succeed");
        assert_eq!(store.load_or_set_aside(2).recovered_store, None);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_store_closed_to_writing_is_never_written() {
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-closed-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = Store::new(&directory);
        fs::write(&store.path, b"{ \"tasks\": [ truncated").unwrap();
        store.writable.store(false, Ordering::Release);

        // Ok, not Err: an error here would fail every command in the app.
        store
            .save(&AppData::default())
            .expect("a closed store should accept the save and do nothing");

        assert_eq!(fs::read(&store.path).unwrap(), b"{ \"tasks\": [ truncated");
        assert!(!store.path.with_extension("json.tmp").exists());
        let entries = fs::read_dir(&directory).unwrap().count();
        assert_eq!(entries, 1, "nothing else was created beside the store");

        let _ = fs::remove_dir_all(directory);
    }

    /// The real double failure: a directory that cannot be written to refuses
    /// the rename and the copy alike. Before the store could be closed, the
    /// save that followed put the directory's permissions back to `rwx------`
    /// on its way in and then replaced the unreadable file with defaults.
    #[cfg(unix)]
    #[test]
    fn a_store_that_cannot_be_set_aside_is_not_saved_over() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "pomodoro-stuck-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&directory).unwrap();
        let store = Store::new(&directory);
        fs::write(&store.path, b"{ \"tasks\": [ truncated").unwrap();
        let set_mode = |mode: u32| {
            fs::set_permissions(&directory, fs::Permissions::from_mode(mode)).unwrap();
        };
        set_mode(0o500);

        // Root ignores directory permissions, so nothing can be made to fail
        // there. Asking the directory is surer than asking for a uid, and
        // covers any other arrangement that lets the write through.
        let probe = directory.join("probe");
        if fs::File::create(&probe).is_ok() {
            eprintln!("directory permissions are not enforced here; skipping");
            set_mode(0o700);
            let _ = fs::remove_dir_all(directory);
            return;
        }

        let data = store.load_or_set_aside(1_234);
        let saved = store.save(&data);
        let after = fs::read(&store.path);
        let mode = fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
        // Restored before any assertion can fail, so the cleanup always works.
        set_mode(0o700);

        assert_eq!(data.recovered_store, Some(String::new()));
        assert_eq!(saved, Ok(()));
        assert_eq!(after.unwrap(), b"{ \"tasks\": [ truncated");
        assert_eq!(mode, 0o500, "the save did not reach for the directory");
        assert!(!store.path.with_extension("json.tmp").exists());
        assert!(!directory.join("pomodoro.unreadable-1234.json").exists());

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
