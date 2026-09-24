//! Recovery points are exercised only in uniquely owned temporary directories.

use super::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "pomodoro-storage-recovery-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn store(&self) -> Store {
        Store::new(&self.0)
    }

    fn backups(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = fs::read_dir(self.0.join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();
        paths
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn first_save_migrates_legacy_data_and_preserves_its_exact_original_bytes_once() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut original = AppData::default();
    original.create_task("Existing work", 3, 10);
    let legacy = serde_json::to_vec_pretty(&original).unwrap();
    fs::write(&store.path, &legacy).unwrap();

    let mut loaded = store.load().unwrap();
    assert_eq!(loaded.tasks, original.tasks);
    assert_eq!(fs::read(&store.path).unwrap(), legacy);
    assert!(!scratch.0.join("backups").exists());

    loaded.create_task("New work", 1, 20);
    store.save(&loaded).unwrap();
    let backups = scratch.backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), legacy);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&store.path).unwrap()).unwrap();
    assert_eq!(saved["schemaVersion"], schema::CURRENT_VERSION);
    assert_eq!(store.load().unwrap().tasks, loaded.tasks);

    store.save(&loaded).unwrap();
    assert_eq!(
        scratch.backups(),
        backups,
        "ordinary saves do not add backups"
    );
}

#[test]
fn failed_migration_backup_keeps_the_legacy_store_unchanged() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let legacy = br#"{"tasks":[],"settings":{"focusMinutes":30}}"#;
    fs::write(&store.path, legacy).unwrap();
    fs::write(scratch.0.join("backups"), b"backup folder is unavailable").unwrap();
    let mut loaded = store.load().unwrap();
    loaded.create_task("Unsaved change", 1, 10);

    assert!(store.save(&loaded).is_err());
    assert_eq!(fs::read(&store.path).unwrap(), legacy);
    assert!(store.migration_pending.load(Ordering::Acquire));
}

#[test]
fn backup_rotation_keeps_the_five_newest_recovery_points_and_unrelated_files() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut data = AppData::default();
    let first = store.backup(&data, 100).unwrap();
    let unrelated = scratch.0.join("backups").join("README.txt");
    fs::write(&unrelated, b"keep this note").unwrap();

    for index in 2..=7 {
        data.create_task(format!("Task {index}"), 1, index);
        store.backup(&data, index * 100).unwrap();
    }

    let backups = scratch.backups();
    assert_eq!(backups.len(), 5);
    assert!(!first.exists());
    let task_counts: Vec<_> = backups
        .iter()
        .map(|path| {
            schema::decode(&fs::read(path).unwrap())
                .unwrap()
                .0
                .tasks
                .len()
        })
        .collect();
    assert_eq!(task_counts, [2, 3, 4, 5, 6]);
    assert_eq!(fs::read(&unrelated).unwrap(), b"keep this note");
}

#[test]
fn recovery_points_created_in_the_same_millisecond_do_not_overwrite_each_other() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut data = AppData::default();
    let first = store.backup(&data, 100).unwrap();
    data.create_task("Later state", 1, 100);
    let second = store.backup(&data, 100).unwrap();

    assert_ne!(first, second);
    assert_eq!(scratch.backups().len(), 2);
    assert!(schema::decode(&fs::read(first).unwrap())
        .unwrap()
        .0
        .tasks
        .is_empty());
    assert_eq!(
        schema::decode(&fs::read(second).unwrap())
            .unwrap()
            .0
            .tasks
            .len(),
        1
    );
}

#[test]
fn rotation_never_removes_the_just_written_backup_when_timestamps_tie() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let directory = scratch.0.join("backups");
    fs::create_dir(&directory).unwrap();
    let bytes = schema::encode(&AppData::default()).unwrap();
    for index in 0..5 {
        // These valid UUID strings sort after every newly generated v4 UUID,
        // making the old UUID-based rotation failure deterministic.
        let name = format!(
            "pomodoro-{:020}-ffffffff-ffff-ffff-ffff-fffffffffff{index}.json",
            100
        );
        fs::write(directory.join(name), &bytes).unwrap();
    }

    let mut data = AppData::default();
    for index in 0..8 {
        data.create_task(format!("State {index}"), 1, 100);
        let latest = store.backup(&data, 100).unwrap();
        assert!(
            latest.exists(),
            "the recovery point just created was removed"
        );
        assert_eq!(
            schema::decode(&fs::read(latest).unwrap()).unwrap().0.tasks,
            data.tasks
        );
        assert_eq!(scratch.backups().len(), 5);
    }
}

#[test]
fn rotation_keeps_the_new_recovery_point_when_the_wall_clock_moves_backwards() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut data = AppData::default();
    for now in 100..105 {
        store.backup(&data, now).unwrap();
    }
    data.create_task("Most recent work", 1, 50);
    let latest = store.backup(&data, 50).unwrap();
    assert_eq!(
        schema::decode(&fs::read(latest).unwrap()).unwrap().0.tasks,
        data.tasks
    );
    assert_eq!(scratch.backups().len(), 5);
}

#[test]
fn a_newer_schema_remains_at_its_original_path_and_cannot_be_overwritten_or_backed_up() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let newer = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": schema::CURRENT_VERSION + 1,
        "tasks": [{"futureField": "valuable work"}]
    }))
    .unwrap();
    fs::write(&store.path, &newer).unwrap();

    let data = store.load_or_set_aside(123);
    assert_eq!(data.recovered_store.as_deref(), Some(""));
    assert!(store.ensure_writable().is_err());
    assert!(store.backup(&data, 124).is_err());
    store.save(&data).unwrap();
    assert_eq!(fs::read(&store.path).unwrap(), newer);
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn backups_and_their_folder_are_private_from_the_first_completed_write() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let store = scratch.store();
    let path = store.backup(&AppData::default(), 10).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), FILE_MODE);
    assert_eq!(mode(&scratch.0.join("backups")), DIRECTORY_MODE);
    assert_eq!(mode(&scratch.0), DIRECTORY_MODE);
}

#[test]
fn failed_atomic_replacement_cleans_up_the_temporary_file() {
    let scratch = Scratch::new();
    let target = scratch.0.join("existing-directory");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("original"), b"keep me").unwrap();

    assert!(write_private(&target, b"replacement").is_err());
    assert_eq!(fs::read(target.join("original")).unwrap(), b"keep me");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}
