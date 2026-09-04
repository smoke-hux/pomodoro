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
