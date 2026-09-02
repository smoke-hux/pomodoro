use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::domain::AppData;

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

        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| format!("could not serialize local data: {error}"))?;
        let mut file = fs::File::create(&temporary)
            .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &self.path).map_err(|error| {
            format!(
                "could not replace {} with saved data: {error}",
                self.path.display()
            )
        })
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
}
