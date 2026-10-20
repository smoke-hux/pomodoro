//! Notification filtering, bounded retention, replacement, and triage.

use super::AppData;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The highest urgency the freedesktop notification specification defines.
pub const MAX_URGENCY: u8 = 2;
/// Newest-first retention cap so the local JSON store cannot grow without
/// bound.
pub const NOTIFICATION_RETENTION: usize = 200;

/// Upper bounds on stored notification text. A sender can put an arbitrarily
/// long string in any of these fields; without a cap one hostile or merely
/// careless app could grow the local store without limit. The limits are far
/// above what a real notification uses, so ordinary text is never touched.
pub const MAX_APP_NAME_CHARS: usize = 128;
pub const MAX_SUMMARY_CHARS: usize = 512;
pub const MAX_BODY_CHARS: usize = 4_096;

/// How recently a row must have been received for a later `Notify` call to be
/// read as an update to it.
///
/// A `replaces_id` is only meaningful for as long as the notification daemon
/// that issued it keeps running: the daemon's counter starts again when it
/// restarts, at login or after a shell crash, while the rows here are
/// persisted for weeks. Without a limit, a call naming id 7 today landed on
/// whatever this app had filed under id 7 last Tuesday and rewrote it. A
/// sender that really is updating — a download counting up, a call still
/// ringing, a music player — refreshes `received_at` with every update, so it
/// never leaves the window however long it goes on.
pub const REPLACE_WINDOW_MS: i64 = 60 * 60 * 1_000;

/// Names Pomodoro's own boundary notifications arrive under. They are dropped
/// before the filter runs, so turning capture on cannot fill the inbox with the
/// app's own "Focus complete" messages.
const SELF_APP_NAMES: &[&str] = &["pomodoro", "app.pomodoro.timer"];

/// True when a notification is one Pomodoro itself sent.
pub fn is_self_notification(app_name: &str) -> bool {
    let name = app_name.trim().to_lowercase();
    SELF_APP_NAMES.contains(&name.as_str())
}

/// Trims a sender-supplied string to `limit` characters, respecting character
/// boundaries so a truncated multi-byte character cannot corrupt the store.
fn truncate_chars(text: String, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((byte_index, _)) => text[..byte_index].to_owned(),
        None => text,
    }
}

/// A desktop notification observed on the session bus.
///
/// `summary` and `body` routinely carry message contents and one-time codes.
/// They are written to the local JSON store and nowhere else: never logged,
/// never transmitted.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DesktopNotification {
    pub id: String,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    pub received_at: i64,
    pub during_focus: bool,
    pub triaged: bool,
    /// The `replaces_id` the sender passed to `Notify`. Non-zero means the call
    /// updates a notification the sender posted earlier, so the update lands on
    /// this record instead of adding another row.
    pub replaces_id: u32,
    /// The task this notification was turned into, if any. Set once so a second
    /// "Turn into task" on the same row cannot create a duplicate, and cleared
    /// when an update brings different words: the link is to a task made from
    /// text this row no longer shows.
    pub task_id: Option<String>,
}

/// Whether the notification monitor is actually running.
///
/// Capture can be switched on in settings and still fail to start — the session
/// bus may refuse `BecomeMonitor`, or there may be no session bus at all. The
/// UI reads this rather than the settings toggle, so it can never claim to be
/// watching when nothing is.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState {
    /// The user has not turned capture on.
    #[default]
    Off,
    /// Capture is on and the monitor thread is coming up.
    Starting,
    /// The monitor is attached to the session bus.
    Active,
    /// Capture is on but the monitor could not run. `detail` says why.
    Failed,
}

/// Runtime health of notification capture, reported to the UI alongside the
/// settings that requested it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureStatus {
    pub state: CaptureState,
    /// A D-Bus error message. Never contains notification text.
    pub detail: String,
}

impl CaptureStatus {
    pub fn off() -> Self {
        Self {
            state: CaptureState::Off,
            detail: String::new(),
        }
    }

    pub fn starting() -> Self {
        Self {
            state: CaptureState::Starting,
            detail: String::new(),
        }
    }

    pub fn active() -> Self {
        Self {
            state: CaptureState::Active,
            detail: String::new(),
        }
    }

    pub fn failed(detail: impl Into<String>) -> Self {
        Self {
            state: CaptureState::Failed,
            detail: detail.into(),
        }
    }
}

/// Declares which notifications are worth keeping. Capture is off until the
/// user opts in.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotificationFilter {
    pub enabled: bool,
    pub min_urgency: u8,
    pub muted_apps: Vec<String>,
    pub priority_apps: Vec<String>,
    pub focus_only: bool,
}

impl NotificationFilter {
    pub fn sanitized(mut self) -> Self {
        self.min_urgency = self.min_urgency.min(MAX_URGENCY);
        self.muted_apps = sanitized_app_list(self.muted_apps);
        self.priority_apps = sanitized_app_list(self.priority_apps);
        self
    }

    /// Decides whether a notification is captured.
    ///
    /// The order below is the contract and must not be reordered:
    ///
    /// 1. capture disabled -> drop everything;
    /// 2. muted app -> drop, even if the app is also listed as priority;
    /// 3. priority app -> keep, ignoring rules 4 and 5;
    /// 4. focus-only while no focus interval runs -> drop;
    /// 5. urgency below the floor -> drop;
    /// 6. otherwise keep.
    pub fn accepts(&self, app_name: &str, urgency: u8, during_focus: bool) -> bool {
        if !self.enabled {
            return false;
        }
        if app_list_contains(&self.muted_apps, app_name) {
            return false;
        }
        if app_list_contains(&self.priority_apps, app_name) {
            return true;
        }
        if self.focus_only && !during_focus {
            return false;
        }
        if urgency < self.min_urgency {
            return false;
        }
        true
    }
}

impl AppData {
    /// Files an observed desktop notification if the filter accepts it.
    ///
    /// Returns `None` when the notification was filtered out, so callers can
    /// skip persisting and broadcasting.
    /// Convenience wrapper for a notification with no `replaces_id`.
    #[cfg(test)]
    pub fn capture_notification(
        &mut self,
        app_name: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
        urgency: u8,
        now_ms: i64,
    ) -> Option<DesktopNotification> {
        self.capture_notify(app_name, summary, body, urgency, 0, now_ms)
    }

    /// Files an observed desktop notification, honouring the sender's
    /// `replaces_id`.
    ///
    /// A non-zero `replaces_id` means the sender is updating a notification it
    /// posted earlier — a download counting up, a call still ringing. Those land
    /// on the existing row rather than adding one per update, so a chatty sender
    /// cannot flood the inbox. Text is truncated and Pomodoro's own boundary
    /// notifications are dropped before the filter is consulted.
    ///
    /// Which row an update belongs to is decided in two steps, both limited to
    /// rows from the same app received within [`REPLACE_WINDOW_MS`]:
    ///
    /// 1. A row already carrying this `replaces_id`.
    /// 2. Failing that, the newest row that was posted as new (`replaces_id`
    ///    0) with the same summary, which is then adopted under this id.
    ///
    /// The second step is a guess, and exists because the first can never
    /// match a sender's first update. A new notification is posted with
    /// `replaces_id` 0; the id the daemon gave it travels back in the method
    /// reply, which a bus monitor filing `Notify` calls does not see. So the
    /// first update arrived naming an id no row had, was filed as a second
    /// row, and only later updates found that one — every updating
    /// notification left its original behind, frozen at "10%". Matching on
    /// the summary is right for the senders this is for, which keep the title
    /// and change the body. It is wrong when one app has two live
    /// notifications with the same summary — two messages from one person —
    /// and updates the older: the newer row is adopted and rewritten instead.
    /// The inbox still ends with the right number of rows and the latest
    /// words; one of them is attributed to the wrong original. A summary that
    /// changes on the first update is not matched at all and adds a row, as
    /// before.
    pub fn capture_notify(
        &mut self,
        app_name: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
        urgency: u8,
        replaces_id: u32,
        now_ms: i64,
    ) -> Option<DesktopNotification> {
        let app_name = truncate_chars(app_name.into().trim().to_owned(), MAX_APP_NAME_CHARS);
        if is_self_notification(&app_name) {
            return None;
        }
        let urgency = urgency.min(MAX_URGENCY);
        let during_focus = self.is_focus_running(now_ms);
        if !self
            .settings
            .notification_filter
            .accepts(&app_name, urgency, during_focus)
        {
            return None;
        }

        let summary = truncate_chars(summary.into(), MAX_SUMMARY_CHARS);
        let body = truncate_chars(body.into(), MAX_BODY_CHARS);

        if replaces_id != 0 {
            let sender = app_name.to_lowercase();
            let replaceable = |notification: &DesktopNotification| {
                notification.app_name.to_lowercase() == sender
                    && now_ms.saturating_sub(notification.received_at) <= REPLACE_WINDOW_MS
            };
            // Newest first, so `position` finds the most recent candidate.
            let position = self
                .notifications
                .iter()
                .position(|notification| {
                    replaceable(notification) && notification.replaces_id == replaces_id
                })
                .or_else(|| {
                    self.notifications.iter().position(|notification| {
                        replaceable(notification)
                            && notification.replaces_id == 0
                            && notification.summary == summary
                    })
                });
            if let Some(position) = position {
                let mut existing = self.notifications.remove(position);
                // Only genuinely new words are worth re-reading. An update that
                // repeats the same text leaves a triaged row triaged and a
                // converted row converted. New words are a new thing to decide
                // about: keeping the link made them show as "already a task"
                // and made converting them return the task written from the
                // old words. That task stays in the list; only the link goes.
                if existing.summary != summary || existing.body != body {
                    existing.triaged = false;
                    existing.task_id = None;
                }
                existing.replaces_id = replaces_id;
                existing.summary = summary;
                existing.body = body;
                existing.urgency = urgency;
                existing.received_at = now_ms;
                existing.during_focus = during_focus;
                self.notifications.insert(0, existing.clone());
                return Some(existing);
            }
        }

        let notification = DesktopNotification {
            id: unique_notification_id(
                now_ms,
                self.notifications
                    .iter()
                    .map(|notification| notification.id.as_str()),
            ),
            app_name,
            summary,
            body,
            urgency,
            received_at: now_ms,
            during_focus,
            triaged: false,
            replaces_id,
            task_id: None,
        };
        self.notifications.insert(0, notification.clone());
        self.notifications.truncate(NOTIFICATION_RETENTION);
        Some(notification)
    }

    /// Turns a captured notification into a task, at most once.
    ///
    /// Returns the task id, whether it was created now or by an earlier call, so
    /// a double click on "Turn into task" cannot leave two identical tasks in
    /// the list.
    pub fn convert_notification_to_task(&mut self, id: &str, now_ms: i64) -> Option<String> {
        let (summary, existing_task) = self
            .notifications
            .iter()
            .find(|notification| notification.id == id)
            .map(|notification| (notification.summary.clone(), notification.task_id.clone()))?;

        if let Some(task_id) = existing_task {
            if self.tasks.iter().any(|task| task.id == task_id) {
                self.set_notification_triaged(id, true);
                return Some(task_id);
            }
        }

        let task = self.create_task(summary, 1, now_ms)?;
        if let Some(notification) = self
            .notifications
            .iter_mut()
            .find(|notification| notification.id == id)
        {
            notification.task_id = Some(task.id.clone());
            notification.triaged = true;
        }
        Some(task.id)
    }

    pub fn set_notification_triaged(&mut self, id: &str, triaged: bool) -> bool {
        let Some(notification) = self
            .notifications
            .iter_mut()
            .find(|notification| notification.id == id)
        else {
            return false;
        };
        notification.triaged = triaged;
        true
    }

    pub fn delete_notification(&mut self, id: &str) -> bool {
        let old_len = self.notifications.len();
        self.notifications
            .retain(|notification| notification.id != id);
        self.notifications.len() != old_len
    }

    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }
}

fn sanitized_app_list(list: Vec<String>) -> Vec<String> {
    let mut sanitized: Vec<String> = Vec::new();
    for entry in list {
        let entry = entry.trim().to_owned();
        if entry.is_empty() {
            continue;
        }
        if sanitized
            .iter()
            .any(|existing| existing.to_lowercase() == entry.to_lowercase())
        {
            continue;
        }
        sanitized.push(entry);
    }
    sanitized
}

/// Application names are compared case-insensitively and ignoring surrounding
/// whitespace, because users type them by hand.
fn app_list_contains(list: &[String], app_name: &str) -> bool {
    let needle = app_name.trim().to_lowercase();
    list.iter()
        .any(|entry| entry.trim().to_lowercase() == needle)
}

fn unique_notification_id<'a>(now_ms: i64, existing: impl Iterator<Item = &'a str>) -> String {
    let existing: HashSet<&str> = existing.collect();
    for counter in 0_u32.. {
        let candidate = format!("notif-{now_ms}-{counter}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite set of existing IDs cannot exhaust all u32 counters")
}
