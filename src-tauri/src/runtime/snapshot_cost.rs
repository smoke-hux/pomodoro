//! Ignored snapshot/persistence cost measurements.

use super::*;
use crate::domain;
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
