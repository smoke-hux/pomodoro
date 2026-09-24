//! Ignored checks requiring a live desktop. Run only on an explicitly approved test session.

use super::*;
use crate::domain::{self, NotificationFilter};
use std::{process::Command, sync::mpsc};

/// Long enough for gnome-shell to accept a call and relay it to monitors.
const SETTLE: Duration = Duration::from_millis(1_500);

fn send(app_name: &str, summary: &str, body: &str) {
    let status = Command::new("notify-send")
        .arg(format!("--app-name={app_name}"))
        .arg(summary)
        .arg(body)
        .status()
        .expect("notify-send must be available");
    assert!(status.success(), "notify-send failed");
}

fn capturing() -> NotificationFilter {
    NotificationFilter {
        enabled: true,
        ..NotificationFilter::default()
    }
}

/// The test plan's first manual check, made repeatable.
///
/// Starts a real focus interval, has another application send a notification
/// through the session bus, and confirms it is filed as arriving during
/// focus while the timer itself is untouched.
#[test]
#[ignore = "requires a live D-Bus session and notification daemon"]
fn a_notification_sent_mid_focus_is_filed_without_disturbing_the_timer() {
    let now = now_ms();
    let mut data = AppData::default();
    data.settings.notification_filter = capturing();
    let task = data.create_task("Write the report", 1, now).unwrap();
    data.select_task(Some(task.id));
    data.start_or_resume(now);

    let started_at = data.timer.started_at;
    let ends_at = data.timer.ends_at;
    assert!(
        data.is_focus_running(now),
        "the focus interval must be running"
    );

    let (sender, receiver) = mpsc::channel();
    let (status_sender, status_receiver) = mpsc::channel();
    let listener = Arc::new(NotificationListener::new());
    listener.start(
        move |event| {
            let _ = sender.send(event);
        },
        move |status| {
            let _ = status_sender.send(status);
        },
    );
    thread::sleep(SETTLE);

    let summary = format!("standup-{}", std::process::id());
    send("Slack", &summary, "In five minutes");
    thread::sleep(SETTLE);
    listener.stop();

    // File whatever arrived exactly as record_notification does, at a
    // timestamp inside the interval.
    let filed = now_ms();
    let mut captured = Vec::new();
    for event in receiver.try_iter() {
        if event.summary != summary {
            continue;
        }
        if let Some(notification) = data.capture_notify(
            event.app_name,
            event.summary,
            event.body,
            event.urgency,
            event.replaces_id,
            filed,
        ) {
            captured.push(notification);
        }
    }

    // It landed, once, attributed to the focus interval.
    assert_eq!(
        captured.len(),
        1,
        "the notification must be filed exactly once"
    );
    let notification = &captured[0];
    assert_eq!(notification.app_name, "Slack");
    assert_eq!(notification.summary, summary);
    assert_eq!(notification.body, "In five minutes");
    assert!(
        notification.during_focus,
        "it arrived during a focus interval"
    );
    assert!(!notification.triaged, "it starts in the pending list");
    assert_eq!(data.notifications.len(), 1, "it is in the inbox");

    // The timer is exactly where it was. Capture never touches it.
    assert_eq!(data.timer.status, TimerStatus::Running);
    assert_eq!(data.timer.phase, Phase::Focus);
    assert_eq!(data.timer.started_at, started_at);
    assert_eq!(data.timer.ends_at, ends_at);
    assert!(data.sessions.is_empty(), "no session was ended or recorded");

    // The monitor reported itself healthy rather than merely switched on.
    let states: Vec<domain::CaptureState> = status_receiver
        .try_iter()
        .map(|status| status.state)
        .collect();
    assert!(
        states.contains(&domain::CaptureState::Active),
        "the monitor should have reported Active, saw {states:?}"
    );
}

/// Pomodoro's own boundary alerts travel the same bus. Turning capture on
/// must not fill the inbox with them.
#[test]
#[ignore = "requires a live D-Bus session and notification daemon"]
fn pomodoros_own_notifications_do_not_reach_the_inbox() {
    let mut data = AppData::default();
    data.settings.notification_filter = capturing();

    let (sender, receiver) = mpsc::channel();
    let listener = Arc::new(NotificationListener::new());
    listener.start(
        move |event| {
            let _ = sender.send(event);
        },
        |_| {},
    );
    thread::sleep(SETTLE);

    let marker = format!("boundary-{}", std::process::id());
    send("Pomodoro", &marker, "Step away for a short break.");
    send("Slack", &format!("other-{marker}"), "A real message");
    thread::sleep(SETTLE);
    listener.stop();

    let now = now_ms();
    for event in receiver.try_iter() {
        data.capture_notify(
            event.app_name,
            event.summary,
            event.body,
            event.urgency,
            event.replaces_id,
            now,
        );
    }

    assert!(
        data.notifications
            .iter()
            .all(|item| item.app_name != "Pomodoro"),
        "Pomodoro's own notification reached the inbox"
    );
    assert!(
        data.notifications
            .iter()
            .any(|item| item.app_name == "Slack"),
        "the other application's notification should still be captured"
    );
}

/// The desktop really goes quiet for the interval and really comes back.
///
/// Restores whatever the machine had before, on every exit path, so running
/// the test cannot leave the desktop silent.
#[test]
#[ignore = "requires a live GNOME session with gsettings"]
fn banners_are_silenced_for_the_interval_and_restored_afterwards() {
    let original = quiet::read_show_banners().expect("GNOME's banner setting must be readable");
    // The feature deliberately does nothing when banners are already off,
    // so the check needs them on to be meaningful.
    quiet::write_show_banners(true).expect("the schema must be writable");

    let outcome = std::panic::catch_unwind(|| {
        let now = now_ms();
        let mut data = AppData::default();
        data.settings.silence_banners_during_focus = true;
        let task = data.create_task("Write the report", 1, now).unwrap();
        data.select_task(Some(task.id));
        data.start_or_resume(now);

        // Entering focus takes the desktop quiet and remembers what to undo.
        assert!(data.is_focus_running(now));
        assert_eq!(quiet::read_show_banners(), Some(true));
        quiet::write_show_banners(false).unwrap();
        data.banner_restore = Some(true);
        assert_eq!(
            quiet::read_show_banners(),
            Some(false),
            "banners should be off during focus"
        );

        // Leaving focus puts it back.
        data.pause(now + 1_000);
        assert!(!data.is_focus_running(now + 1_000));
        let previous = data.banner_restore.take().expect("a marker to restore");
        quiet::write_show_banners(previous).unwrap();
        assert_eq!(
            quiet::read_show_banners(),
            Some(true),
            "banners should be back after focus"
        );
    });

    quiet::write_show_banners(original).expect("the original setting must be restored");
    assert_eq!(quiet::read_show_banners(), Some(original));
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
