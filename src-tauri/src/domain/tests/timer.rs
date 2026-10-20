use super::*;

#[test]
fn expiry_follows_four_focus_rounds_then_a_long_break() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    let task = data.create_task("Write tests", 4, 0).unwrap();
    assert!(data.select_task(Some(task.id.clone())));

    let mut now = 0;
    for completed_focuses in 1..=4 {
        assert_eq!(data.timer.phase, Phase::Focus);
        assert_eq!(data.timer.status, TimerStatus::Idle);
        assert!(data.start_or_resume(now));
        now += 60_000;
        assert_eq!(data.tick(now), Some(Phase::Focus));
        assert_eq!(data.timer.completed_in_cycle, completed_focuses);
        assert_eq!(data.tasks[0].completed_pomodoros, completed_focuses);

        let expected_break = if completed_focuses == 4 {
            Phase::LongBreak
        } else {
            Phase::ShortBreak
        };
        assert_eq!(data.timer.phase, expected_break);
        assert_eq!(data.timer.status, TimerStatus::Running);

        now += 60_000;
        assert_eq!(data.tick(now), Some(expected_break));
        assert_eq!(data.timer.phase, Phase::Focus);
        assert_eq!(data.timer.status, TimerStatus::Idle);
    }

    assert_eq!(data.timer.completed_in_cycle, 0);
    assert_eq!(data.sessions.len(), 8);
    assert!(data
        .sessions
        .iter()
        .all(|session| session.outcome == SessionOutcome::Completed));
}

#[test]
fn pause_preserves_displayed_seconds_and_resume_uses_a_new_deadline() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    assert!(data.start_or_resume(1_000));

    assert!(data.pause(13_345));
    assert_eq!(data.timer.status, TimerStatus::Paused);
    assert_eq!(data.timer.remaining_seconds, 48);
    assert_eq!(data.timer.ends_at, None);
    assert_eq!(data.snapshot(50_000).timer.remaining_seconds, 48);

    assert!(data.start_or_resume(50_000));
    assert_eq!(data.timer.ends_at, Some(98_000));
    assert_eq!(data.tick(97_999), None);
    assert_eq!(data.timer.remaining_seconds, 1);
    assert_eq!(data.tick(98_000), Some(Phase::Focus));
}

#[test]
fn quitting_as_a_focus_runs_out_completes_it_but_starts_no_break() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    assert!(data.settings.auto_start_breaks);
    data.start_or_resume(0);

    // Quit a moment after the deadline, before the timer thread noticed.
    data.pause_to_quit(60_010);
    assert_eq!(data.timer.phase, Phase::ShortBreak);
    assert_eq!(data.timer.status, TimerStatus::Idle);
    assert_eq!(data.timer.completed_in_cycle, 1);
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].phase, Phase::Focus);
    assert_eq!(data.sessions[0].outcome, SessionOutcome::Completed);

    // Launched ten minutes later, there is no break to record as taken.
    assert!(!data.recover_at_launch(660_010));
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.timer.status, TimerStatus::Idle);
}

#[test]
fn quitting_before_the_deadline_pauses_what_is_left() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    data.start_or_resume(0);

    data.pause_to_quit(20_000);
    assert_eq!(data.timer.phase, Phase::Focus);
    assert_eq!(data.timer.status, TimerStatus::Paused);
    assert_eq!(data.timer.remaining_seconds, 40);
    assert!(data.sessions.is_empty());
}

#[test]
fn skipping_focus_records_the_skip_but_never_awards_focus_credit() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    let task = data.create_task("Ship feature", 2, 10).unwrap();
    data.select_task(Some(task.id));
    data.start_or_resume(100);

    assert_eq!(data.skip(30_100), Phase::ShortBreak);
    assert_eq!(data.timer.completed_in_cycle, 0);
    assert_eq!(data.tasks[0].completed_pomodoros, 0);
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].outcome, SessionOutcome::Skipped);
    assert_eq!(data.sessions[0].duration_seconds, 30);
    assert_eq!(data.timer.status, TimerStatus::Running);
}

#[test]
fn ticking_an_expired_phase_is_idempotent() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    data.start_or_resume(0);

    assert_eq!(data.tick(60_000), Some(Phase::Focus));
    let after_first_tick = data.clone();
    assert_eq!(data.tick(60_000), None);
    assert_eq!(data, after_first_tick);
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.timer.completed_in_cycle, 1);
}

#[test]
fn settings_only_resize_idle_and_future_phases() {
    let mut data = AppData::default();
    data.start_or_resume(0);

    let mut changed = data.settings.clone();
    changed.focus_minutes = 45;
    changed.short_break_minutes = 9;
    data.update_settings(changed.clone());
    assert_eq!(data.timer.duration_seconds, 1_500);

    data.pause(10_000);
    changed.focus_minutes = 40;
    data.update_settings(changed.clone());
    assert_eq!(data.timer.duration_seconds, 1_500);

    data.skip(20_000);
    assert_eq!(data.timer.phase, Phase::ShortBreak);
    assert_eq!(data.timer.status, TimerStatus::Running);
    assert_eq!(data.timer.duration_seconds, 9 * 60);

    changed.short_break_minutes = 7;
    data.update_settings(changed);
    assert_eq!(data.timer.duration_seconds, 9 * 60);

    data.tick(20_000 + 9 * 60 * 1_000);
    assert_eq!(data.timer.phase, Phase::Focus);
    assert_eq!(data.timer.status, TimerStatus::Idle);
    assert_eq!(data.timer.duration_seconds, 40 * 60);

    let mut final_settings = data.settings.clone();
    final_settings.focus_minutes = 35;
    data.update_settings(final_settings);
    assert_eq!(data.timer.duration_seconds, 35 * 60);
    assert_eq!(data.timer.remaining_seconds, 35 * 60);
}

#[test]
fn completed_active_task_keeps_attribution_then_clears_before_next_focus() {
    let mut data = AppData::default();
    let mut settings = one_minute_settings();
    settings.auto_start_focus = true;
    data.update_settings(settings);
    let task = data.create_task("Finish early", 1, 0).unwrap();
    data.select_task(Some(task.id.clone()));
    data.start_or_resume(0);

    assert!(data.set_task_done(&task.id, true, 1_000));
    assert_eq!(data.timer.active_task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(data.tick(60_000), Some(Phase::Focus));
    assert_eq!(data.sessions[0].task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(data.sessions[0].task_title.as_deref(), Some("Finish early"));
    assert_eq!(data.tasks[0].completed_pomodoros, 1);

    assert_eq!(data.tick(120_000), Some(Phase::ShortBreak));
    assert_eq!(data.timer.phase, Phase::Focus);
    assert_eq!(data.timer.status, TimerStatus::Idle);
    assert_eq!(data.timer.active_task_id, None);
}

#[test]
fn a_focus_interval_that_ran_out_is_no_longer_focus() {
    // The status stays Running between expiry and the next tick, up to half
    // a second later. A notification arriving in that gap belongs to the
    // break the user is already in, not to the focus that just ended.
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    running_focus(&mut data);
    let expires_at = data.timer.ends_at.expect("a running timer has a deadline");

    assert!(data.is_focus_running(expires_at - 1));
    assert!(!data.is_focus_running(expires_at));
    assert!(!data.is_focus_running(expires_at + 5_000));

    let late = data
        .capture_notification("Slack", "Standup", "Now", 1, expires_at + 100)
        .expect("the notification is still captured");
    assert!(!late.during_focus);
    // The timer has not been ticked yet, so this really is the gap.
    assert_eq!(data.timer.status, TimerStatus::Running);
    assert_eq!(data.timer.phase, Phase::Focus);
}

#[test]
fn a_completion_noticed_late_is_recorded_as_ending_at_its_deadline() {
    let mut data = AppData::default();
    data.update_settings(one_minute_settings());
    data.start_or_resume(1_000);

    // The machine slept through the deadline and woke three hours later.
    let noticed = 1_000 + 3 * 60 * 60 * 1_000;
    assert_eq!(data.tick(noticed), Some(Phase::Focus));
    let session = &data.sessions[0];
    assert_eq!(session.outcome, SessionOutcome::Completed);
    assert_eq!(session.started_at, 1_000);
    assert_eq!(session.ended_at, 61_000);
    assert_eq!(session.duration_seconds, 60);
    // The break that follows starts when it was noticed, not in the past.
    assert_eq!(data.timer.started_at, Some(noticed));

    // After a pause the deadline moves, and the record follows it.
    let mut paused = AppData::default();
    paused.update_settings(one_minute_settings());
    paused.start_or_resume(0);
    paused.pause(30_000);
    paused.start_or_resume(100_000);
    assert_eq!(paused.tick(500_000), Some(Phase::Focus));
    assert_eq!(paused.sessions[0].started_at, 0);
    assert_eq!(paused.sessions[0].ended_at, 130_000);

    // A clock that went backwards cannot file a session in the future, or
    // one that ends before it starts.
    let mut skewed = AppData::default();
    skewed.update_settings(one_minute_settings());
    skewed.start_or_resume(100_000);
    skewed.timer.ends_at = Some(90_000);
    assert_eq!(skewed.tick(95_000), Some(Phase::Focus));
    assert_eq!(skewed.sessions[0].ended_at, 90_000);
    assert!(skewed.sessions[0].started_at <= skewed.sessions[0].ended_at);

    // What the user ends by hand ended when they ended it.
    let mut skipped = AppData::default();
    skipped.update_settings(one_minute_settings());
    skipped.start_or_resume(0);
    skipped.skip(20_000);
    assert_eq!(skipped.sessions[0].ended_at, 20_000);
    let mut abandoned = AppData::default();
    abandoned.update_settings(one_minute_settings());
    abandoned.start_or_resume(0);
    abandoned.reset(25_000);
    assert_eq!(abandoned.sessions[0].ended_at, 25_000);
}

/// What an orderly quit does, followed by a launch the next morning.
#[test]
fn a_timer_paused_at_quit_completes_nothing_however_late_the_relaunch() {
    let mut data = AppData::default();
    running_focus(&mut data);
    assert!(data.pause(5 * 60 * 1_000));

    // Through the store and back, as a relaunch would have it.
    let json = serde_json::to_string(&data).unwrap();
    let mut relaunched: AppData = serde_json::from_str(&json).unwrap();
    let next_morning = 16 * 60 * 60 * 1_000;

    assert!(!relaunched.recover_at_launch(next_morning));
    assert_eq!(relaunched.tick(next_morning), None);
    assert!(relaunched.sessions.is_empty());
    assert_eq!(relaunched.tasks[0].completed_pomodoros, 0);
    assert_eq!(relaunched.timer.status, TimerStatus::Paused);
    assert_eq!(relaunched.timer.phase, Phase::Focus);
    assert_eq!(relaunched.timer.remaining_seconds, 20 * 60);
    assert_eq!(
        relaunched.snapshot(next_morning).timer.remaining_seconds,
        20 * 60
    );
}

#[test]
fn a_phase_that_ran_out_while_the_app_was_closed_is_settled_without_starting_the_next() {
    let mut data = AppData::default();
    assert!(data.settings.auto_start_breaks);
    running_focus(&mut data);
    let deadline = data.timer.ends_at.unwrap();
    let next_morning = deadline + 12 * 60 * 60 * 1_000;

    assert!(data.recover_at_launch(next_morning));
    // The work was done, and is recorded when it was done.
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].outcome, SessionOutcome::Completed);
    assert_eq!(data.sessions[0].ended_at, deadline);
    assert_eq!(data.tasks[0].completed_pomodoros, 1);
    assert_eq!(data.timer.completed_in_cycle, 1);
    // But nobody is taking this break: it waits to be started.
    assert_eq!(data.timer.phase, Phase::ShortBreak);
    assert_eq!(data.timer.status, TimerStatus::Idle);
    assert_eq!(data.timer.started_at, None);
    assert_eq!(data.timer.ends_at, None);

    // Nothing is left for the tick that follows to announce.
    assert!(!data.recover_at_launch(next_morning));
    assert_eq!(data.tick(next_morning), None);
    assert_eq!(data.sessions.len(), 1);

    // The same from a break into a focus that would have started itself.
    let mut from_break = AppData::default();
    from_break.settings.auto_start_focus = true;
    let task = from_break.create_task("Carry on", 1, 0).unwrap();
    from_break.select_task(Some(task.id));
    from_break.set_phase(Phase::ShortBreak, 0);
    from_break.start_or_resume(0);
    assert!(from_break.recover_at_launch(next_morning));
    assert_eq!(from_break.timer.phase, Phase::Focus);
    assert_eq!(from_break.timer.status, TimerStatus::Idle);
}

#[test]
fn launch_leaves_alone_a_timer_that_is_not_due_or_not_running() {
    // Still counting: the deadline is wall-clock time and has not come.
    let mut running = AppData::default();
    running_focus(&mut running);
    let deadline = running.timer.ends_at;
    assert!(!running.recover_at_launch(10 * 60 * 1_000));
    assert_eq!(running.timer.status, TimerStatus::Running);
    assert_eq!(running.timer.ends_at, deadline);
    assert_eq!(running.timer.started_at, Some(0));
    assert!(running.sessions.is_empty());
    assert_eq!(
        running.snapshot(10 * 60 * 1_000).timer.remaining_seconds,
        900
    );

    let mut paused = AppData::default();
    running_focus(&mut paused);
    paused.pause(60_000);
    let before = paused.clone();
    assert!(!paused.recover_at_launch(i64::MAX / 2));
    assert_eq!(paused, before);

    let mut idle = AppData::default();
    let before = idle.clone();
    assert!(!idle.recover_at_launch(i64::MAX / 2));
    assert_eq!(idle, before);
}

#[test]
fn session_history_is_capped_and_the_snapshot_ships_only_the_recent_week() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let day = 24 * 60 * 60 * 1_000;

    // Fill past the cap, one skipped break a day, oldest first.
    for index in 0..(SESSION_RETENTION as i64 + 25) {
        let now = index * day;
        data.set_phase(Phase::ShortBreak, now);
        data.start_or_resume(now);
        data.skip(now + 1_000);
    }
    assert_eq!(
        data.sessions.len(),
        SESSION_RETENTION,
        "the oldest are dropped"
    );
    assert_eq!(
        data.sessions[0].started_at,
        25 * day,
        "dropping happens at the old end, never the new one"
    );

    // The snapshot carries only what the UI can show.
    let latest = data.sessions.last().unwrap().started_at;
    let snapshot = data.snapshot(latest);
    assert!(
        snapshot.sessions.len() < 10,
        "got {}",
        snapshot.sessions.len()
    );
    assert!(snapshot
        .sessions
        .iter()
        .all(|session| session.started_at >= latest - SNAPSHOT_SESSION_WINDOW_MS));
    assert!(snapshot.sessions.contains(data.sessions.last().unwrap()));

    // Everything else is intact, and the countdown is normalised.
    assert_eq!(snapshot.tasks, data.tasks);
    assert_eq!(snapshot.settings, data.settings);
    assert_eq!(snapshot.notifications, data.notifications);
    assert_eq!(snapshot.timer.status, data.timer.status);
    // The full history is still on disk.
    assert_eq!(data.sessions.len(), SESSION_RETENTION);
}
