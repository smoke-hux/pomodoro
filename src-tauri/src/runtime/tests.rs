//! Reconciliation, deferred saves, wake-ups, and orderly shutdown.

use super::*;
use std::{fs, path::PathBuf, time::Instant};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pomodoro-runtime-{name}-{}-{}",
        std::process::id(),
        now_ms()
    ))
}

/// A store whose every save fails: its data directory is a file.
fn unwritable(name: &str) -> (Store, PathBuf) {
    let blocker = scratch(name);
    fs::write(&blocker, b"not a directory").unwrap();
    (Store::new(&blocker), blocker)
}

/// A focus on one task that began `elapsed_ms` before `now`.
fn focus_begun(now: i64, elapsed_ms: i64) -> AppData {
    let mut data = AppData::default();
    let task = data.create_task("Write the report", 1, 0).unwrap();
    data.select_task(Some(task.id));
    data.start_or_resume(now - elapsed_ms);
    data
}

const TWENTY_SIX_MINUTES: i64 = 26 * 60 * 1_000;

#[test]
fn a_nudge_that_lands_before_the_wait_begins_is_not_lost() {
    let state = RuntimeState::new(AppData::default(), Store::new(scratch("nudge")));
    // The gap: the thread has decided how long to sleep, and is not yet
    // asleep.
    state.nudge();

    let began = Instant::now();
    state.wait_for_work(Duration::from_secs(5));
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "slept {:?} through a nudge",
        began.elapsed()
    );

    // One nudge is one wake-up: the next wait runs its time.
    let began = Instant::now();
    state.wait_for_work(Duration::from_millis(60));
    assert!(began.elapsed() >= Duration::from_millis(60));
}

#[test]
fn a_nudge_during_the_wait_ends_it() {
    let state = Arc::new(RuntimeState::new(
        AppData::default(),
        Store::new(scratch("nudge-during")),
    ));
    let nudger = Arc::clone(&state);
    let began = Instant::now();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        nudger.nudge();
    });
    state.wait_for_work(Duration::from_secs(5));
    assert!(began.elapsed() < Duration::from_secs(1));
    handle.join().unwrap();
}

#[test]
fn the_wait_is_the_nearest_thing_that_needs_doing() {
    let now = 1_000_000;
    assert_eq!(wait_for(None, 0, now), IDLE_WAKE);
    assert_eq!(
        wait_for(Some(now + 400), 0, now),
        Duration::from_millis(400)
    );
    assert_eq!(wait_for(Some(now + 60_000), 0, now), RUNNING_WAKE);
    assert_eq!(wait_for(Some(now - 5), 0, now), Duration::ZERO);
    // A notification filed half a second ago is written in a second and a
    // half.
    assert_eq!(wait_for(None, now - 500, now), Duration::from_millis(1_500));
    assert_eq!(wait_for(None, now - 10_000, now), Duration::ZERO);

    // After a failed save the next attempt is a retry away — not zero,
    // which is what an overdue mark left in place used to give.
    let retry = Duration::from_millis(SAVE_RETRY_MS as u64);
    assert_eq!(wait_for(None, retry_mark(now), now), retry.min(IDLE_WAKE));
    assert_eq!(
        wait_for(None, retry_mark(now), now + 10_000),
        Duration::from_millis(SAVE_RETRY_MS as u64 - 10_000)
    );
    assert_ne!(
        retry_mark(-SAVE_RETRY_MS),
        0,
        "0 would mean nothing to save"
    );
}

#[test]
fn a_flush_that_fails_is_tried_again_later_rather_than_at_once() {
    let (store, blocker) = unwritable("flush");
    let state = RuntimeState::new(AppData::default(), store);
    let now = now_ms();
    state.mark_notifications_dirty(now - 5_000);
    assert_eq!(state.next_wait(now), Duration::ZERO, "the flush is overdue");

    state.flush_notifications(now, false);
    let wait = state.next_wait(now);
    assert!(
        wait >= Duration::from_millis(SAVE_RETRY_MS as u64).min(IDLE_WAKE),
        "a failed flush left a wait of {wait:?}"
    );
    // Still dirty, so the change is not forgotten...
    let mark = state.notifications_dirty_since.load(Ordering::Acquire);
    assert_eq!(mark, retry_mark(now));
    // ...and the flush agrees with the wait that it is not yet due: a
    // notification arriving meanwhile does not pull it forward either.
    state.mark_notifications_dirty(now + 1_000);
    state.flush_notifications(now + 1_000, false);
    assert_eq!(
        state.notifications_dirty_since.load(Ordering::Acquire),
        mark
    );

    // Once the store can be written, the retry lands and clears the mark.
    fs::remove_file(&blocker).unwrap();
    state.flush_notifications(now + SAVE_RETRY_MS, false);
    assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
    assert!(state.store.load().is_ok());
    let _ = fs::remove_dir_all(blocker);
}

#[test]
fn a_later_save_that_works_clears_a_pending_retry() {
    let (store, blocker) = unwritable("clears");
    let state = RuntimeState::new(AppData::default(), store);
    let now = now_ms();
    state.mark_notifications_dirty(now - 5_000);
    state.flush_notifications(now, false);
    assert_ne!(state.notifications_dirty_since.load(Ordering::Acquire), 0);

    fs::remove_file(&blocker).unwrap();
    let change = apply(&state, now + 10, |data, now| {
        data.create_task("Anything", 1, now);
        Ok(())
    });
    assert_eq!(change.result, Ok(()));
    assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
    let _ = fs::remove_dir_all(blocker);
}

#[test]
fn a_phase_that_ended_is_kept_when_the_action_then_refuses() {
    let directory = scratch("refused");
    let now = now_ms();
    let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), Store::new(&directory));

    let change = apply(&state, now, |data, now| {
        data.create_task("   ", 1, now)
            .ok_or_else(|| "Enter a task name.".to_string())?;
        Ok(())
    });

    // The person still hears that their title was refused...
    assert_eq!(change.result, Err("Enter a task name.".to_string()));
    // ...and the focus that finished is announced, shown and on disk.
    assert_eq!(change.completed, Some(Phase::Focus));
    let snapshot = change.snapshot.expect("the completed phase is published");
    assert_eq!(snapshot.timer.phase, Phase::ShortBreak);
    assert_eq!(snapshot.sessions.len(), 1);
    let saved = state.store.load().unwrap();
    assert_eq!(saved.sessions.len(), 1);
    assert_eq!(saved.tasks[0].completed_pomodoros, 1);
    assert_eq!(saved.tasks.len(), 1, "the refused task was not added");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn an_action_that_refuses_with_nothing_else_going_on_changes_nothing() {
    let directory = scratch("refused-idle");
    let state = RuntimeState::new(AppData::default(), Store::new(&directory));

    let change = apply(&state, now_ms(), |_, _| Err("No.".to_string()));

    assert_eq!(change.result, Err("No.".to_string()));
    assert!(change.snapshot.is_none());
    assert_eq!(change.completed, None);
    assert!(!directory.exists(), "nothing was saved");
}

#[test]
fn a_save_that_fails_still_publishes_and_is_retried() {
    let (store, blocker) = unwritable("save-fails");
    let now = now_ms();
    let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);

    // A user action: applied in memory, published, and the error surfaced.
    let change = apply(&state, now, |data, now| {
        data.create_task("Second task", 1, now);
        Ok(())
    });
    assert!(change.result.is_err());
    assert_eq!(change.completed, Some(Phase::Focus));
    let snapshot = change
        .snapshot
        .expect("memory is the truth; it is published");
    assert_eq!(snapshot.tasks.len(), 2);
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(
        state.notifications_dirty_since.load(Ordering::Acquire),
        retry_mark(now),
        "the write is owed, and will be retried"
    );

    // Both failing: the action's error is the one the person can act on.
    let (store, second_blocker) = unwritable("both-fail");
    let both = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);
    let change = apply(&both, now, |_, _| Err("Enter a task name.".to_string()));
    assert_eq!(change.result, Err("Enter a task name.".to_string()));
    assert!(change.snapshot.is_some());
    assert_ne!(both.notifications_dirty_since.load(Ordering::Acquire), 0);

    let _ = fs::remove_file(blocker);
    let _ = fs::remove_file(second_blocker);
}

#[test]
fn a_phase_end_is_published_even_when_it_cannot_be_saved() {
    let (store, blocker) = unwritable("reconcile");
    let now = now_ms();
    let state = RuntimeState::new(focus_begun(now, TWENTY_SIX_MINUTES), store);

    let change = reconcile(&state, now);
    assert!(change.result.is_err());
    assert_eq!(change.completed, Some(Phase::Focus));
    assert_eq!(
        change.snapshot.expect("published").timer.phase,
        Phase::ShortBreak
    );
    assert_eq!(
        state.notifications_dirty_since.load(Ordering::Acquire),
        retry_mark(now)
    );

    // Nothing further happened, so nothing further is attempted: a store
    // that will not save is not hammered from here either.
    let again = reconcile(&state, now + 1_000);
    assert_eq!(again.result, Ok(()));
    assert!(again.snapshot.is_none());
    let _ = fs::remove_file(blocker);
}

#[test]
fn nothing_silences_the_desktop_once_shutdown_has_begun() {
    assert!(may_silence(true, false));
    assert!(!may_silence(true, true));
    assert!(!may_silence(false, false));
    assert!(!may_silence(false, true));
}

#[test]
fn quitting_pauses_a_running_interval_saves_it_and_happens_once() {
    let directory = scratch("quit");
    let now = now_ms();
    let state = RuntimeState::new(focus_begun(now, 5 * 60 * 1_000), Store::new(&directory));
    state.mark_notifications_dirty(now);

    state.shut_down(now);

    assert!(state.shutting_down.load(Ordering::SeqCst));
    assert_eq!(state.notifications_dirty_since.load(Ordering::Acquire), 0);
    let saved = state.store.load().unwrap();
    assert_eq!(saved.timer.status, TimerStatus::Paused);
    assert_eq!(saved.timer.remaining_seconds, 20 * 60);
    assert!(saved.sessions.is_empty());
    // The thread it nudged has nothing to sleep for.
    let began = Instant::now();
    state.wait_for_work(Duration::from_secs(5));
    assert!(began.elapsed() < Duration::from_secs(1));

    // Exit is reported more than once; only the first call acts.
    state.data.lock().unwrap().start_or_resume(now + 1_000);
    state.shut_down(now + 2_000);
    assert_eq!(
        state.data.lock().unwrap().timer.status,
        TimerStatus::Running,
        "a second shutdown did its work again"
    );
    let _ = fs::remove_dir_all(directory);
}
