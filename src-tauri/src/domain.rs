//! Persisted application state and the public domain facade.
//!
//! Timer transitions, task references, settings, and notification capture are
//! implemented in sibling modules; their types remain available as domain::*.

mod capture;
mod settings;
mod tasks;
mod timer;

pub use capture::*;
pub use settings::*;
pub use tasks::*;
pub use timer::*;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// How far back the sessions in a [`AppData::snapshot`] reach. The ledger shows
/// today and the metrics look back a week, so shipping every record ever
/// written to the UI on every update was cost for nothing. Eight days rather
/// than seven so a DST change or a late night cannot clip the seventh day.
pub const SNAPSHOT_SESSION_WINDOW_MS: i64 = 8 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppData {
    pub settings: Settings,
    pub timer: TimerState,
    pub tasks: Vec<FocusTask>,
    pub interruptions: Vec<Interruption>,
    pub sessions: Vec<SessionRecord>,
    /// Newest first, capped at [`NOTIFICATION_RETENTION`].
    pub notifications: Vec<DesktopNotification>,
    /// Set while Pomodoro has the desktop's notification banners turned off,
    /// holding the value to put back. Persisted deliberately: if the app is
    /// killed mid-focus the next launch reads this and restores the desktop
    /// rather than leaving it silent forever.
    pub banner_restore: Option<bool>,
    /// Runtime only. Serialized so the UI can read it, never read back from
    /// disk, because a monitor that was running last time says nothing about
    /// whether one is running now.
    #[serde(skip_deserializing)]
    pub capture_status: CaptureStatus,
    /// Set for this run only, when the store on disk could not be read and was
    /// set aside: the name it was kept under, or empty if it could not be
    /// kept. Never read back, so the notice does not outlive the launch that
    /// caused it.
    #[serde(skip_deserializing)]
    pub recovered_store: Option<String>,
}

impl Default for AppData {
    fn default() -> Self {
        let settings = Settings::default();
        let timer = TimerState::for_phase(Phase::Focus, &settings);
        Self {
            settings,
            timer,
            tasks: Vec::new(),
            interruptions: Vec::new(),
            sessions: Vec::new(),
            notifications: Vec::new(),
            banner_restore: None,
            capture_status: CaptureStatus::off(),
            recovered_store: None,
        }
    }
}

impl AppData {
    /// Produces a display snapshot with a current countdown. Phase completion is
    /// deliberately handled by [`Self::tick`] so callers can notify and persist
    /// exactly once when a transition occurs.
    ///
    /// Only the last [`SNAPSHOT_SESSION_WINDOW_MS`] of session history is
    /// included: that is all the UI reads, and the full history stays on disk.
    pub fn snapshot(&self, now_ms: i64) -> Self {
        let oldest = now_ms.saturating_sub(SNAPSHOT_SESSION_WINDOW_MS);
        let mut timer = self.timer.clone();
        timer.normalize_remaining(now_ms);
        Self {
            settings: self.settings.clone(),
            timer,
            tasks: self.tasks.clone(),
            interruptions: self.interruptions.clone(),
            sessions: self
                .sessions
                .iter()
                .filter(|session| session.started_at >= oldest)
                .cloned()
                .collect(),
            notifications: self.notifications.clone(),
            banner_restore: self.banner_restore,
            capture_status: self.capture_status.clone(),
            recovered_store: self.recovered_store.clone(),
        }
    }
}

fn unique_id<'a>(prefix: &str, now_ms: i64, existing: impl Iterator<Item = &'a str>) -> String {
    let existing: HashSet<&str> = existing.collect();
    let base = format!("{prefix}-{now_ms}");
    if !existing.contains(base.as_str()) {
        return base;
    }

    for suffix in 1_u32.. {
        let candidate = format!("{base}-{suffix}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite set of existing IDs cannot exhaust all u32 suffixes")
}

#[cfg(test)]
mod tests;
