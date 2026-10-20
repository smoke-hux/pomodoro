use super::*;

#[test]
fn filter_precedence_follows_the_numbered_contract() {
    // 1. Disabled capture drops everything, including critical urgency and
    //    apps the user marked as priority.
    let disabled = NotificationFilter {
        enabled: false,
        priority_apps: vec!["Signal".to_string()],
        ..NotificationFilter::default()
    };
    assert!(!disabled.accepts("Signal", 2, true));
    assert!(!disabled.accepts("Anything", 2, true));

    // 2. A muted app is dropped, and mute wins over priority when the same
    //    app appears in both lists.
    let muted = NotificationFilter {
        muted_apps: vec!["Slack".to_string()],
        priority_apps: vec!["Slack".to_string()],
        ..capturing_filter()
    };
    assert!(!muted.accepts("Slack", 2, true));
    // Matching is case-insensitive and ignores surrounding whitespace.
    assert!(!muted.accepts("  sLaCk ", 2, true));
    assert!(muted.accepts("Signal", 1, true));

    // 3. A priority app is kept even when rules 4 and 5 would drop it.
    let priority = NotificationFilter {
        priority_apps: vec!["Signal".to_string()],
        focus_only: true,
        min_urgency: 2,
        ..capturing_filter()
    };
    assert!(priority.accepts("Signal", 0, false));
    assert!(priority.accepts("SIGNAL", 0, false));
    assert!(!priority.accepts("Slack", 0, false));

    // 4. focus_only drops anything that arrives outside a focus interval.
    let focus_only = NotificationFilter {
        focus_only: true,
        ..capturing_filter()
    };
    assert!(!focus_only.accepts("Slack", 2, false));
    assert!(focus_only.accepts("Slack", 2, true));

    // 5. Urgency below the floor is dropped; at or above it is kept.
    let floor = NotificationFilter {
        min_urgency: 1,
        ..capturing_filter()
    };
    assert!(!floor.accepts("Slack", 0, true));
    assert!(floor.accepts("Slack", 1, true));
    assert!(floor.accepts("Slack", 2, true));

    // 6. Otherwise keep.
    assert!(capturing_filter().accepts("Slack", 0, false));
}

#[test]
fn capture_defaults_to_off_and_records_focus_context() {
    let mut data = AppData::default();
    assert!(!data.settings.notification_filter.enabled);
    assert!(data
        .capture_notification("Slack", "Standup", "In five minutes", 1, 500)
        .is_none());
    assert!(data.notifications.is_empty());

    data.settings.notification_filter = capturing_filter();
    let captured = data
        .capture_notification("Slack", "Standup", "In five minutes", 1, 500)
        .expect("an enabled filter with no rules keeps everything");
    assert_eq!(captured.id, "notif-500-0");
    assert_eq!(captured.app_name, "Slack");
    assert_eq!(captured.urgency, 1);
    assert_eq!(captured.received_at, 500);
    assert!(!captured.during_focus);
    assert!(!captured.triaged);

    let mut focused = AppData::default();
    focused.settings.notification_filter = capturing_filter();
    running_focus(&mut focused);
    let during = focused
        .capture_notification("Slack", "Standup", "Now", 1, 1_000)
        .expect("capture during focus");
    assert!(during.during_focus);
}

#[test]
fn focus_only_capture_stops_the_moment_the_interval_expires() {
    let mut data = AppData::default();
    data.settings.notification_filter = NotificationFilter {
        focus_only: true,
        ..capturing_filter()
    };
    running_focus(&mut data);
    let expires_at = data.timer.ends_at.expect("a running timer has a deadline");

    assert!(data
        .capture_notification("Slack", "During", "", 1, expires_at - 1)
        .is_some());
    assert!(data
        .capture_notification("Slack", "After", "", 1, expires_at + 1)
        .is_none());
}

#[test]
fn pomodoros_own_notifications_are_never_captured() {
    let mut data = AppData::default();
    data.settings.notification_filter = NotificationFilter {
        // Even naming itself a priority app cannot get it in.
        priority_apps: vec!["Pomodoro".to_string()],
        ..capturing_filter()
    };

    assert!(data
        .capture_notification("Pomodoro", "Focus complete", "Step away", 1, 10)
        .is_none());
    assert!(data
        .capture_notification("  pomodoro  ", "Break complete", "", 1, 11)
        .is_none());
    assert!(data
        .capture_notification("app.pomodoro.timer", "Focus complete", "", 1, 12)
        .is_none());
    assert!(data.notifications.is_empty());

    // A different app whose name merely contains "pomodoro" is not us.
    assert!(data
        .capture_notification("Pomodoro Tracker", "Hello", "", 1, 13)
        .is_some());
}

#[test]
fn oversized_notification_text_is_truncated_on_a_character_boundary() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    // Multi-byte throughout, so a byte-wise cut would split a character and
    // corrupt the store.
    let captured = data
        .capture_notification(
            "é".repeat(MAX_APP_NAME_CHARS + 10),
            "字".repeat(MAX_SUMMARY_CHARS + 10),
            "😀".repeat(MAX_BODY_CHARS + 10),
            1,
            10,
        )
        .expect("an oversized notification is kept, just shortened");

    assert_eq!(captured.app_name.chars().count(), MAX_APP_NAME_CHARS);
    assert_eq!(captured.summary.chars().count(), MAX_SUMMARY_CHARS);
    assert_eq!(captured.body.chars().count(), MAX_BODY_CHARS);
    // Round-tripping proves nothing was cut mid-character.
    let json = serde_json::to_string(&captured).unwrap();
    let restored: DesktopNotification = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, captured);
}

#[test]
fn an_update_lands_on_the_notification_it_replaces() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    let first = data
        .capture_notify("Transmission", "Downloading", "10%", 1, 7, 100)
        .expect("the first sighting creates a row");
    let updated = data
        .capture_notify("Transmission", "Downloading", "90%", 1, 7, 200)
        .expect("the update lands on the same row");

    assert_eq!(data.notifications.len(), 1);
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.body, "90%");
    assert_eq!(updated.received_at, 200);

    // A different sender reusing the same id is a different notification.
    data.capture_notify("Firefox", "Downloading", "10%", 1, 7, 300)
        .expect("another app's id 7 is its own");
    assert_eq!(data.notifications.len(), 2);

    // replaces_id 0 always means "new", however often it is used.
    data.capture_notify("Transmission", "Seeding", "a", 1, 0, 400)
        .unwrap();
    data.capture_notify("Transmission", "Seeding", "b", 1, 0, 401)
        .unwrap();
    assert_eq!(data.notifications.len(), 4);
}

#[test]
fn an_update_reopens_a_triaged_row_only_when_the_words_changed() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    let first = data
        .capture_notify("Signal", "Alice", "See you at six", 1, 3, 100)
        .unwrap();
    assert!(data.set_notification_triaged(&first.id, true));

    // The same words again: the user has already dealt with this.
    let repeat = data
        .capture_notify("Signal", "Alice", "See you at six", 1, 3, 200)
        .unwrap();
    assert!(repeat.triaged);

    // New words deserve another look.
    let changed = data
        .capture_notify("Signal", "Alice", "Make it seven", 1, 3, 300)
        .unwrap();
    assert!(!changed.triaged);
}

#[test]
fn turning_a_notification_into_a_task_twice_makes_one_task() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let captured = data
        .capture_notification("Thunderbird", "Re: brief review", "body", 1, 10)
        .unwrap();

    let first = data
        .convert_notification_to_task(&captured.id, 20)
        .expect("the first conversion creates a task");
    let second = data
        .convert_notification_to_task(&captured.id, 30)
        .expect("the second returns the task the first made");

    assert_eq!(first, second);
    assert_eq!(data.tasks.len(), 1);
    assert_eq!(data.tasks[0].title, "Re: brief review");
    assert_eq!(
        data.notifications[0].task_id.as_deref(),
        Some(first.as_str())
    );
    assert!(data.notifications[0].triaged);

    // Deleting the task releases the link, so the notification can be turned
    // into a task again rather than pointing at something that is gone.
    assert!(data.delete_task(&first));
    assert_eq!(data.notifications[0].task_id, None);
    let third = data
        .convert_notification_to_task(&captured.id, 40)
        .expect("a released notification converts again");
    assert_ne!(third, first);
    assert_eq!(data.tasks.len(), 1);

    assert!(data
        .convert_notification_to_task("notif-missing", 50)
        .is_none());
}

#[test]
fn an_update_with_new_words_is_no_longer_the_task_made_from_the_old_ones() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let first = data
        .capture_notify("Signal", "Alice", "Send me the report", 1, 3, 100)
        .unwrap();
    let task = data.convert_notification_to_task(&first.id, 150).unwrap();

    // The same words again: still that task, still dealt with.
    let repeat = data
        .capture_notify("Signal", "Alice", "Send me the report", 1, 3, 200)
        .unwrap();
    assert_eq!(repeat.task_id.as_deref(), Some(task.as_str()));
    assert!(repeat.triaged);

    // Different words are a different thing to decide about.
    let changed = data
        .capture_notify("Signal", "Alice", "Also book the room", 1, 3, 300)
        .unwrap();
    assert_eq!(changed.id, first.id);
    assert_eq!(changed.task_id, None);
    assert!(!changed.triaged);
    assert_eq!(data.notifications[0].task_id, None);
    // The task written from the old words is not taken back.
    assert_eq!(data.tasks.len(), 1);

    let second_task = data.convert_notification_to_task(&first.id, 400).unwrap();
    assert_ne!(second_task, task);
    assert_eq!(data.tasks.len(), 2);
}

#[test]
fn an_id_from_before_the_replace_window_belongs_to_another_notification() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let old = data
        .capture_notify("Signal", "Alice", "See you at six", 1, 7, 1_000)
        .unwrap();

    // The daemon restarted and handed id 7 out again, days later.
    let later = 1_000 + REPLACE_WINDOW_MS + 1;
    let unrelated = data
        .capture_notify("Signal", "Bob", "Lunch?", 1, 7, later)
        .unwrap();
    assert_ne!(unrelated.id, old.id);
    assert_eq!(data.notifications.len(), 2);
    assert_eq!(data.notifications[1].summary, "Alice");
    assert_eq!(data.notifications[1].body, "See you at six");

    // On the edge of the window it is still the same notification.
    let mut edge = AppData::default();
    edge.settings.notification_filter = capturing_filter();
    edge.capture_notify("Signal", "Alice", "a", 1, 7, 1_000)
        .unwrap();
    edge.capture_notify("Signal", "Alice", "b", 1, 7, 1_000 + REPLACE_WINDOW_MS)
        .unwrap();
    assert_eq!(edge.notifications.len(), 1);

    // A sender that keeps updating never leaves the window, however long
    // it goes on, because each update is the new "received".
    let mut long = AppData::default();
    long.settings.notification_filter = capturing_filter();
    for step in 0..5 {
        long.capture_notify(
            "Transmission",
            "Downloading",
            format!("{step}"),
            1,
            9,
            step * (REPLACE_WINDOW_MS - 1),
        )
        .unwrap();
    }
    assert_eq!(long.notifications.len(), 1);
    assert_eq!(long.notifications[0].body, "4");
}

#[test]
fn the_first_update_adopts_the_row_its_sender_posted_as_new() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let posted = data
        .capture_notify("Transmission", "Downloading", "10%", 1, 0, 100)
        .unwrap();
    assert_eq!(posted.replaces_id, 0);

    // Case-insensitive on the app name, as the id match is.
    let first_update = data
        .capture_notify("transmission", "Downloading", "50%", 1, 42, 200)
        .expect("the first update is captured");
    assert_eq!(data.notifications.len(), 1, "no duplicate is left behind");
    assert_eq!(first_update.id, posted.id);
    assert_eq!(first_update.body, "50%");
    assert_eq!(first_update.replaces_id, 42);
    assert_eq!(first_update.received_at, 200);

    let second_update = data
        .capture_notify("Transmission", "Downloading", "90%", 1, 42, 300)
        .unwrap();
    assert_eq!(data.notifications.len(), 1);
    assert_eq!(second_update.id, posted.id);
    assert_eq!(second_update.body, "90%");

    // An adopted row follows the same rules as any replaced one.
    let mut triaged = AppData::default();
    triaged.settings.notification_filter = capturing_filter();
    let row = triaged
        .capture_notify("Signal", "Alice", "Send me the report", 1, 0, 100)
        .unwrap();
    triaged.convert_notification_to_task(&row.id, 150).unwrap();
    let adopted = triaged
        .capture_notify("Signal", "Alice", "Also book the room", 1, 5, 200)
        .unwrap();
    assert_eq!(adopted.id, row.id);
    assert_eq!(adopted.task_id, None);
    assert!(!adopted.triaged);
}

#[test]
fn an_update_does_not_adopt_a_row_it_has_no_reason_to_claim() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    // A different summary.
    data.capture_notify("Transmission", "Downloading", "10%", 1, 0, 100)
        .unwrap();
    data.capture_notify("Transmission", "Finished", "ubuntu.iso", 1, 42, 200)
        .unwrap();
    assert_eq!(data.notifications.len(), 2);
    assert_eq!(data.notifications[1].replaces_id, 0);
    assert_eq!(data.notifications[1].body, "10%");

    // Another app's row with the same summary.
    data.capture_notify("Firefox", "Downloading", "1%", 1, 43, 300)
        .unwrap();
    assert_eq!(data.notifications.len(), 3);

    // A row already known under another id.
    let mut known = AppData::default();
    known.settings.notification_filter = capturing_filter();
    known
        .capture_notify("Transmission", "Downloading", "10%", 1, 41, 100)
        .unwrap();
    known
        .capture_notify("Transmission", "Downloading", "20%", 1, 42, 200)
        .unwrap();
    assert_eq!(known.notifications.len(), 2);

    // A row from outside the window: whatever id it was given is long gone.
    let mut old = AppData::default();
    old.settings.notification_filter = capturing_filter();
    old.capture_notify("Transmission", "Downloading", "10%", 1, 0, 100)
        .unwrap();
    old.capture_notify(
        "Transmission",
        "Downloading",
        "50%",
        1,
        42,
        100 + REPLACE_WINDOW_MS + 1,
    )
    .unwrap();
    assert_eq!(old.notifications.len(), 2);
    assert_eq!(old.notifications[1].body, "10%");
    assert_eq!(old.notifications[1].replaces_id, 0);

    // With two candidates the newest is the one adopted.
    let mut two = AppData::default();
    two.settings.notification_filter = capturing_filter();
    two.capture_notify("Signal", "Alice", "first", 1, 0, 100)
        .unwrap();
    let newer = two
        .capture_notify("Signal", "Alice", "second", 1, 0, 200)
        .unwrap();
    let update = two
        .capture_notify("Signal", "Alice", "second, edited", 1, 8, 300)
        .unwrap();
    assert_eq!(update.id, newer.id);
    assert_eq!(two.notifications.len(), 2);
    assert_eq!(two.notifications[1].body, "first");
}

#[test]
fn retention_keeps_the_newest_two_hundred_notifications() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    for index in 0..(NOTIFICATION_RETENTION as i64 + 50) {
        data.capture_notification("Slack", format!("Message {index}"), "", 1, index)
            .expect("every notification passes the empty filter");
    }

    assert_eq!(data.notifications.len(), NOTIFICATION_RETENTION);
    // Newest first: the last one captured leads, the oldest 50 are gone.
    assert_eq!(data.notifications[0].summary, "Message 249");
    assert_eq!(data.notifications[0].received_at, 249);
    assert_eq!(
        data.notifications[NOTIFICATION_RETENTION - 1].summary,
        "Message 50"
    );
    assert!(data
        .notifications
        .iter()
        .all(|notification| notification.received_at >= 50));
}

#[test]
fn notification_ids_stay_unique_within_a_millisecond() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let first = data.capture_notification("A", "one", "", 1, 42).unwrap();
    let second = data.capture_notification("A", "two", "", 1, 42).unwrap();
    assert_eq!(first.id, "notif-42-0");
    assert_eq!(second.id, "notif-42-1");
}

#[test]
fn ten_thousand_replacement_updates_never_grow_the_inbox() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();

    for index in 0..10_000 {
        data.capture_notify(
            "Software Updater",
            format!("Download {index}"),
            "",
            1,
            77,
            index,
        )
        .expect("every update passes the capture filter");
    }

    assert_eq!(data.notifications.len(), 1);
    assert_eq!(data.notifications[0].summary, "Download 9999");
    assert_eq!(data.notifications[0].received_at, 9_999);
    assert_eq!(data.notifications[0].replaces_id, 77);
}

#[test]
fn notification_triage_conversion_and_removal() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let captured = data
        .capture_notification("Slack", "Review the PR", "body", 1, 10)
        .unwrap();

    assert!(data.set_notification_triaged(&captured.id, true));
    assert!(data.notifications[0].triaged);
    assert!(data.set_notification_triaged(&captured.id, false));
    assert!(!data.notifications[0].triaged);
    assert!(!data.set_notification_triaged("notif-missing", true));

    assert!(data.delete_notification(&captured.id));
    assert!(data.notifications.is_empty());
    assert!(!data.delete_notification(&captured.id));

    data.capture_notification("Slack", "Another", "", 1, 11)
        .unwrap();
    data.capture_notification("Slack", "And another", "", 1, 12)
        .unwrap();
    data.clear_notifications();
    assert!(data.notifications.is_empty());
}

#[test]
fn urgency_above_the_specified_range_is_clamped() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing_filter();
    let captured = data.capture_notification("A", "s", "b", 200, 1).unwrap();
    assert_eq!(captured.urgency, MAX_URGENCY);

    let sanitized = NotificationFilter {
        min_urgency: 200,
        ..capturing_filter()
    }
    .sanitized();
    assert_eq!(sanitized.min_urgency, MAX_URGENCY);
}

#[test]
fn filter_sanitization_trims_and_deduplicates_app_lists() {
    let sanitized = NotificationFilter {
        muted_apps: vec![
            "  Slack ".to_string(),
            "slack".to_string(),
            "   ".to_string(),
            "Signal".to_string(),
        ],
        ..capturing_filter()
    }
    .sanitized();
    assert_eq!(sanitized.muted_apps, vec!["Slack", "Signal"]);
}
