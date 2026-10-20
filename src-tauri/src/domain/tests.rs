use super::*;

mod capture;
mod tasks;
mod timer;

fn one_minute_settings() -> Settings {
    Settings {
        focus_minutes: 1,
        short_break_minutes: 1,
        long_break_minutes: 1,
        ..Settings::default()
    }
}

#[test]
fn defaults_match_the_product_contract() {
    let data = AppData::default();
    assert_eq!(data.settings.focus_minutes, 25);
    assert_eq!(data.settings.short_break_minutes, 5);
    assert_eq!(data.settings.long_break_minutes, 15);
    assert_eq!(data.settings.rounds_before_long_break, 4);
    assert!(data.settings.auto_start_breaks);
    assert!(!data.settings.auto_start_focus);
    assert_eq!(data.timer.duration_seconds, 1_500);
    assert_eq!(data.timer.remaining_seconds, 1_500);
}

#[test]
fn serde_uses_the_typescript_camel_case_contract() {
    let value = serde_json::to_value(AppData::default()).unwrap();
    assert_eq!(value["timer"]["phase"], "focus");
    assert_eq!(value["timer"]["status"], "idle");
    assert_eq!(value["timer"]["durationSeconds"], 1_500);
    assert_eq!(value["settings"]["shortBreakMinutes"], 5);
    assert_eq!(value["settings"]["theme"], "system");
}

fn capturing_filter() -> NotificationFilter {
    NotificationFilter {
        enabled: true,
        ..NotificationFilter::default()
    }
}

fn running_focus(data: &mut AppData) {
    data.create_task("Focus on something", 1, 0);
    let id = data.tasks[0].id.clone();
    data.select_task(Some(id));
    data.start_or_resume(0);
    assert!(data.is_focus_running(0));
}

#[test]
fn capture_status_is_runtime_only_and_never_read_back_from_disk() {
    let mut data = AppData::default();
    assert_eq!(data.capture_status.state, CaptureState::Off);

    data.capture_status = CaptureStatus::failed("the session bus refused BecomeMonitor");
    let json = serde_json::to_value(&data).unwrap();
    // The UI needs to see it...
    assert_eq!(json["captureStatus"]["state"], "failed");
    assert_eq!(
        json["captureStatus"]["detail"],
        "the session bus refused BecomeMonitor"
    );

    // ...but a monitor that ran last time says nothing about this run.
    let reloaded: AppData = serde_json::from_value(json).unwrap();
    assert_eq!(reloaded.capture_status, CaptureStatus::off());
}

#[test]
fn a_store_without_the_notification_fields_still_loads() {
    // A pomodoro.json written before this feature existed: no
    // `notifications` array and no `settings.notificationFilter`.
    let legacy = r#"{
        "settings": {
            "focusMinutes": 30,
            "shortBreakMinutes": 5,
            "longBreakMinutes": 15,
            "roundsBeforeLongBreak": 4,
            "autoStartBreaks": true,
            "autoStartFocus": false,
            "notifications": true,
            "sound": true,
            "theme": "dark"
        },
        "timer": {
            "phase": "focus",
            "status": "idle",
            "durationSeconds": 1800,
            "remainingSeconds": 1800,
            "startedAt": null,
            "endsAt": null,
            "activeTaskId": null,
            "completedInCycle": 2
        },
        "tasks": [
            {
                "id": "task-1",
                "title": "Existing work",
                "estimate": 3,
                "completedPomodoros": 1,
                "done": false,
                "createdAt": 100,
                "completedAt": null
            }
        ],
        "interruptions": [
            {
                "id": "interruption-1",
                "text": "Phone call",
                "category": "external",
                "capturedAt": 200,
                "handled": false,
                "taskId": "task-1"
            }
        ],
        "sessions": [
            {
                "id": "session-1",
                "phase": "focus",
                "taskId": "task-1",
                "taskTitle": "Existing work",
                "durationSeconds": 1800,
                "startedAt": 100,
                "endedAt": 1900,
                "outcome": "completed"
            }
        ]
    }"#;

    let data: AppData = serde_json::from_str(legacy).expect("an older store must still load");

    // Existing tasks, interruptions and history survive untouched.
    assert_eq!(data.tasks.len(), 1);
    assert_eq!(data.tasks[0].title, "Existing work");
    assert_eq!(data.tasks[0].completed_pomodoros, 1);
    assert_eq!(data.interruptions.len(), 1);
    assert_eq!(data.interruptions[0].text, "Phone call");
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].outcome, SessionOutcome::Completed);
    assert_eq!(data.settings.focus_minutes, 30);
    assert_eq!(data.settings.theme, ThemePreference::Dark);
    assert_eq!(data.timer.completed_in_cycle, 2);

    // The new fields take their safe defaults: capture off, nothing filed.
    assert!(data.notifications.is_empty());
    assert_eq!(
        data.settings.notification_filter,
        NotificationFilter::default()
    );
    assert!(!data.settings.notification_filter.enabled);

    // A partially upgraded store (filter present, notifications missing)
    // also loads.
    let partial = r#"{"settings":{"notificationFilter":{"enabled":true,"minUrgency":2}}}"#;
    let data: AppData = serde_json::from_str(partial).expect("partial store must load");
    assert!(data.settings.notification_filter.enabled);
    assert_eq!(data.settings.notification_filter.min_urgency, 2);
    assert!(data.settings.notification_filter.muted_apps.is_empty());
    assert!(data.notifications.is_empty());
    assert_eq!(data.settings.focus_minutes, 25);
}

#[test]
fn notification_serde_matches_the_typescript_contract() {
    let mut data = AppData::default();
    data.settings.notification_filter = NotificationFilter {
        enabled: true,
        min_urgency: 2,
        muted_apps: vec!["Slack".to_string()],
        priority_apps: vec!["Signal".to_string()],
        focus_only: true,
    };
    data.capture_notification("Signal", "Alice", "See you at six", 2, 7)
        .unwrap();

    let value = serde_json::to_value(&data).unwrap();
    let filter = &value["settings"]["notificationFilter"];
    assert_eq!(filter["enabled"], true);
    assert_eq!(filter["minUrgency"], 2);
    assert_eq!(filter["mutedApps"][0], "Slack");
    assert_eq!(filter["priorityApps"][0], "Signal");
    assert_eq!(filter["focusOnly"], true);

    let notification = &value["notifications"][0];
    assert_eq!(notification["id"], "notif-7-0");
    assert_eq!(notification["appName"], "Signal");
    assert_eq!(notification["summary"], "Alice");
    assert_eq!(notification["body"], "See you at six");
    assert_eq!(notification["urgency"], 2);
    assert_eq!(notification["receivedAt"], 7);
    assert_eq!(notification["duringFocus"], false);
    assert_eq!(notification["triaged"], false);

    // Round trips without loss.
    let restored: AppData = serde_json::from_value(value).unwrap();
    assert_eq!(restored, data);
}
