//! Observes desktop notifications on the D-Bus session bus.
//!
//! The listener becomes a bus monitor (`org.freedesktop.DBus.Monitoring`) and
//! watches `org.freedesktop.Notifications.Notify` method calls made by other
//! applications.
//!
//! # Privacy
//!
//! Summaries and bodies routinely carry message contents and one-time codes.
//! Nothing in this module writes them to stdout, stderr, a log, or the network.
//! The only sink is the caller-supplied closure, which files them in the local
//! JSON store. Error paths deliberately report only the D-Bus error, never the
//! payload.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

use zbus::{
    blocking::{Connection, MessageIterator},
    message::{Message, Type},
    zvariant::{OwnedValue, Value},
    MatchRule,
};

use crate::domain::CaptureStatus;

/// The eight arguments of `org.freedesktop.Notifications.Notify` (`susssasa{sv}i`),
/// named so the deserialisation below stays readable.
type NotifyArguments = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, OwnedValue>,
    i32,
);

const NOTIFICATIONS_INTERFACE: &str = "org.freedesktop.Notifications";
const NOTIFY_MEMBER: &str = "Notify";

/// GNOME relays each `Notify` call a second time, roughly two milliseconds
/// later. A one-second window absorbs that without swallowing a genuine repeat
/// the user sends deliberately.
pub const DEDUP_WINDOW_MS: i64 = 1_000;

/// Bounds the dedup memory even if the clock jumps backwards.
const DEDUP_CAPACITY: usize = 64;

/// The freedesktop default when a notification carries no `urgency` hint.
const DEFAULT_URGENCY: u8 = 1;

/// One observed `Notify` call, stripped of everything the app does not need.
///
/// Deliberately not `Debug`: the summary and body carry private content, so
/// `{:?}` and `dbg!` must not be able to reach them.
#[derive(Clone, Eq, PartialEq)]
pub struct NotifyEvent {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: u8,
    /// The id the sender is replacing. Zero means this is a new notification;
    /// anything else means the sender is updating one it posted earlier.
    pub replaces_id: u32,
}

impl NotifyEvent {
    /// Identity used to recognise the relayed second copy. The relay preserves
    /// the app name, summary, body and `replaces_id` but adds hints of its own,
    /// so those four fields are the only stable key.
    ///
    /// Fields are length-prefixed so text containing the separator cannot make
    /// two different notifications look alike.
    ///
    /// The key contains notification text and must never be logged.
    pub fn dedup_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.app_name.len(),
            self.app_name,
            self.summary.len(),
            self.summary,
            self.body.len(),
            self.body,
            self.replaces_id
        )
    }
}

/// Drops the second sighting of a notification seen within a short window.
///
/// Deliberately not `Debug`: its keys embed notification text.
pub struct Deduplicator {
    window_ms: i64,
    recent: VecDeque<(String, i64)>,
}

impl Deduplicator {
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms,
            recent: VecDeque::new(),
        }
    }

    /// Returns true the first time a key is seen, false for repeats inside the
    /// window.
    pub fn accept(&mut self, key: String, now_ms: i64) -> bool {
        while let Some((_, seen_at)) = self.recent.front() {
            if now_ms.saturating_sub(*seen_at) > self.window_ms {
                self.recent.pop_front();
            } else {
                break;
            }
        }

        if self.recent.iter().any(|(seen, _)| *seen == key) {
            return false;
        }

        self.recent.push_back((key, now_ms));
        while self.recent.len() > DEDUP_CAPACITY {
            self.recent.pop_front();
        }
        true
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::new(DEDUP_WINDOW_MS)
    }
}

/// Owns the monitoring thread and its bus connection.
///
/// `start` and `stop` are idempotent and safe to call from any thread. A
/// generation counter retires a thread whose connection has been superseded, so
/// a rapid disable/enable cannot leave two monitors filing the same
/// notification.
#[derive(Debug, Default)]
pub struct NotificationListener {
    generation: AtomicU64,
    running: AtomicBool,
    connection: Mutex<Option<Connection>>,
}

impl NotificationListener {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts monitoring unless it is already running. Each accepted, deduped
    /// notification is handed to `sink` on the monitoring thread.
    ///
    /// `report` is called whenever the monitor's health changes, so the UI can
    /// say "watching" only when something really is. Turning capture on is a
    /// request, not a guarantee: the session bus can refuse `BecomeMonitor`, and
    /// there may be no session bus at all.
    pub fn start<F, G>(self: &Arc<Self>, sink: F, report: G)
    where
        F: Fn(NotifyEvent) + Send + 'static,
        G: Fn(CaptureStatus) + Send + Sync + 'static,
    {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }

        // Shared because both the monitoring thread and the spawn-failure path
        // below have to be able to report.
        let report = Arc::new(report);
        report(CaptureStatus::starting());

        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let listener = Arc::clone(self);
        let thread_report = Arc::clone(&report);
        thread::Builder::new()
            .name("notification-monitor".to_string())
            .spawn(move || {
                let report = thread_report;
                let outcome = listener.monitor(generation, sink, report.as_ref());
                let superseded = listener.generation.load(Ordering::Acquire) != generation;
                if !superseded {
                    listener.running.store(false, Ordering::Release);
                    if let Ok(mut held) = listener.connection.lock() {
                        *held = None;
                    }
                }
                match outcome {
                    Err(error) => {
                        // Only the D-Bus failure is reported. No notification
                        // content reaches this branch.
                        eprintln!("notification capture stopped: {error}");
                        if !superseded {
                            report(CaptureStatus::failed(error.to_string()));
                        }
                    }
                    // The loop ends without an error when the bus closes the
                    // connection under us. That is still a capture that is no
                    // longer happening, so it must not be reported as healthy.
                    Ok(()) if !superseded => {
                        report(CaptureStatus::failed(
                            "the session bus closed the connection".to_string(),
                        ));
                    }
                    Ok(()) => {}
                }
            })
            .map(|_| ())
            .unwrap_or_else(|error| {
                self.running.store(false, Ordering::Release);
                eprintln!("could not start notification capture: {error}");
                report(CaptureStatus::failed(error.to_string()));
            });
    }

    /// Stops monitoring. Closing the connection ends the blocking iterator, so
    /// the thread exits promptly rather than waiting for the next notification.
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.running.store(false, Ordering::Release);
        let held = self
            .connection
            .lock()
            .ok()
            .and_then(|mut connection| connection.take());
        if let Some(connection) = held {
            let _ = connection.close();
        }
    }

    fn monitor<F, G>(&self, generation: u64, sink: F, report: &G) -> zbus::Result<()>
    where
        F: Fn(NotifyEvent),
        G: Fn(CaptureStatus),
    {
        let connection = Connection::session()?;
        {
            let mut held = self
                .connection
                .lock()
                .map_err(|_| zbus::Error::Failure("listener state is unavailable".to_string()))?;
            if self.generation.load(Ordering::Acquire) != generation {
                return Ok(());
            }
            *held = Some(connection.clone());
        }

        let rule = MatchRule::builder()
            .msg_type(Type::MethodCall)
            .interface(NOTIFICATIONS_INTERFACE)?
            .member(NOTIFY_MEMBER)?
            .build();
        // `org.freedesktop.DBus.Monitoring.BecomeMonitor` is called directly
        // rather than through `fdo::MonitoringProxy`, whose generated blocking
        // variant does not exist because the trait method consumes `self`.
        //
        // After this call the connection may only receive; sending anything
        // further on it is an error.
        connection.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(vec![rule.to_string()], 0_u32),
        )?;

        report(CaptureStatus::active());

        let mut deduplicator = Deduplicator::new(DEDUP_WINDOW_MS);
        for message in MessageIterator::from(connection) {
            if self.generation.load(Ordering::Acquire) != generation {
                break;
            }
            let Ok(message) = message else {
                break;
            };
            let Some(event) = parse_notify(&message) else {
                continue;
            };
            if !deduplicator.accept(event.dedup_key(), chrono::Utc::now().timestamp_millis()) {
                continue;
            }
            sink(event);
        }
        Ok(())
    }
}

/// Reads the eight-argument `Notify` payload (`susssasa{sv}i`). Anything that
/// does not match the signature is ignored rather than reported, so a
/// non-conforming sender cannot spam the console with payload fragments.
fn parse_notify(message: &Message) -> Option<NotifyEvent> {
    let header = message.header();
    if header.message_type() != Type::MethodCall {
        return None;
    }
    if header.interface().map(|name| name.as_str()) != Some(NOTIFICATIONS_INTERFACE) {
        return None;
    }
    if header.member().map(|name| name.as_str()) != Some(NOTIFY_MEMBER) {
        return None;
    }

    let body = message.body();
    let (app_name, replaces_id, _app_icon, summary, text, _actions, hints, _expire_timeout): NotifyArguments =
        body.deserialize().ok()?;

    let urgency = hints
        .get("urgency")
        .and_then(|hint| urgency_from_value(hint))
        .unwrap_or(DEFAULT_URGENCY)
        .min(2);

    Some(NotifyEvent {
        app_name,
        summary,
        body: text,
        urgency,
        replaces_id,
    })
}

/// The urgency hint is a byte per the specification, but senders in the wild
/// use other integer types, so accept any integer that fits.
fn urgency_from_value(value: &Value<'_>) -> Option<u8> {
    match value {
        Value::U8(urgency) => Some(*urgency),
        Value::I16(urgency) => u8::try_from(*urgency).ok(),
        Value::U16(urgency) => u8::try_from(*urgency).ok(),
        Value::I32(urgency) => u8::try_from(*urgency).ok(),
        Value::U32(urgency) => u8::try_from(*urgency).ok(),
        Value::I64(urgency) => u8::try_from(*urgency).ok(),
        Value::U64(urgency) => u8::try_from(*urgency).ok(),
        Value::Value(inner) => urgency_from_value(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(app_name: &str, summary: &str, body: &str) -> NotifyEvent {
        NotifyEvent {
            app_name: app_name.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            urgency: 1,
            replaces_id: 0,
        }
    }

    #[test]
    fn the_relayed_second_copy_of_a_notification_is_dropped() {
        // GNOME delivers each Notify twice: sender -> gnome-shell, then
        // gnome-shell -> the next consumer, about two milliseconds apart. The
        // relayed copy carries extra hints, so only app/summary/body are equal.
        let first = event("Signal", "Alice", "See you at six");
        let relayed = NotifyEvent {
            urgency: 1,
            ..first.clone()
        };

        let mut deduplicator = Deduplicator::new(DEDUP_WINDOW_MS);
        assert!(deduplicator.accept(first.dedup_key(), 10_000));
        assert!(!deduplicator.accept(relayed.dedup_key(), 10_002));
    }

    #[test]
    fn deduplication_is_scoped_to_the_window_and_to_the_exact_content() {
        let mut deduplicator = Deduplicator::new(DEDUP_WINDOW_MS);
        let repeated = event("Signal", "Alice", "See you at six");

        assert!(deduplicator.accept(repeated.dedup_key(), 0));
        assert!(!deduplicator.accept(repeated.dedup_key(), DEDUP_WINDOW_MS));
        // Past the window the same text is a genuinely new notification.
        assert!(deduplicator.accept(repeated.dedup_key(), DEDUP_WINDOW_MS + 1));

        // Differing in any one field is a different notification.
        assert!(deduplicator.accept(event("Slack", "Alice", "See you at six").dedup_key(), 1));
        assert!(deduplicator.accept(event("Signal", "Bob", "See you at six").dedup_key(), 1));
        assert!(deduplicator.accept(event("Signal", "Alice", "Make it seven").dedup_key(), 1));
    }

    #[test]
    fn dedup_keys_cannot_be_forged_by_embedding_the_separator() {
        let split = event("a\u{1f}b", "c", "d");
        let joined = event("a", "b\u{1f}c", "d");
        assert_ne!(split.dedup_key(), joined.dedup_key());
    }

    #[test]
    fn urgency_hints_accept_any_integer_encoding() {
        assert_eq!(urgency_from_value(&Value::U8(2)), Some(2));
        assert_eq!(urgency_from_value(&Value::U32(0)), Some(0));
        assert_eq!(urgency_from_value(&Value::I32(1)), Some(1));
        assert_eq!(urgency_from_value(&Value::I32(-1)), None);
        assert_eq!(urgency_from_value(&Value::Str("high".into())), None);
    }

    /// Exercises the real session bus. Ignored by default because it needs a
    /// running notification daemon; run with
    /// `cargo test -- --ignored --nocapture live_session_bus`.
    #[test]
    #[ignore = "requires a live D-Bus session and notification daemon"]
    fn live_session_bus_captures_each_notification_once() {
        use std::sync::mpsc;

        let (sender, receiver) = mpsc::channel();
        let listener = Arc::new(NotificationListener::new());
        listener.start(
            move |event| {
                let _ = sender.send(event);
            },
            |_| {},
        );
        thread::sleep(std::time::Duration::from_millis(1_500));

        let send = |summary: &str| {
            let status = std::process::Command::new("notify-send")
                .arg("--app-name=PomodoroSelfTest")
                .arg("--urgency=critical")
                .arg(summary)
                .arg("selftest-body")
                .status()
                .expect("notify-send must be available");
            assert!(status.success());
        };

        let summary = format!("pomodoro-selftest-{}", std::process::id());
        send(&summary);

        thread::sleep(std::time::Duration::from_millis(1_500));
        listener.stop();

        let captured: Vec<NotifyEvent> = receiver
            .try_iter()
            .filter(|event| event.summary == summary)
            .collect();
        // GNOME relays the call, so the bus carries two copies; exactly one
        // must survive deduplication.
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].app_name, "PomodoroSelfTest");
        assert_eq!(captured[0].body, "selftest-body");
        assert_eq!(captured[0].urgency, 2);

        // A stopped listener observes nothing.
        let after_stop = format!("{summary}-after-stop");
        send(&after_stop);
        thread::sleep(std::time::Duration::from_millis(1_500));
        assert!(receiver.try_iter().count() == 0);

        // Re-enabling capture works, so the connection was really released.
        let (sender, receiver) = mpsc::channel();
        listener.start(
            move |event| {
                let _ = sender.send(event);
            },
            |_| {},
        );
        thread::sleep(std::time::Duration::from_millis(1_500));
        let restarted = format!("{summary}-restarted");
        send(&restarted);
        thread::sleep(std::time::Duration::from_millis(1_500));
        listener.stop();
        assert_eq!(
            receiver
                .try_iter()
                .filter(|event| event.summary == restarted)
                .count(),
            1
        );
    }

    #[test]
    fn stopping_a_listener_that_never_started_is_safe() {
        let listener = Arc::new(NotificationListener::new());
        listener.stop();
        listener.stop();
    }
}
