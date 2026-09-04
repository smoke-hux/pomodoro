use serde::{Deserialize, Serialize};

const MIN_MINUTES: u32 = 1;
const MAX_MINUTES: u32 = 24 * 60;
const MIN_ROUNDS: u32 = 1;
const MAX_ROUNDS: u32 = 24;

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn is_break(self) -> bool {
        matches!(self, Self::ShortBreak | Self::LongBreak)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerStatus {
    #[default]
    Idle,
    Running,
    Paused,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionOutcome {
    #[default]
    Completed,
    Skipped,
    Abandoned,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// How the remaining time is drawn. Each face answers "how much is left?" in a
/// different way — read, proportion, count, glance, or words — rather than being
/// a decorative skin over the same numerals.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerFace {
    #[default]
    Digits,
    Ring,
    Pips,
    Bar,
    Words,
    Analog,
    Vessel,
    Arc,
    Blocks,
    Orbit,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InterruptionCategory {
    #[default]
    Internal,
    External,
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
    /// "Turn into task" on the same row cannot create a duplicate.
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub rounds_before_long_break: u32,
    pub auto_start_breaks: bool,
    pub auto_start_focus: bool,
    pub notifications: bool,
    pub sound: bool,
    pub theme: ThemePreference,
    pub timer_face: TimerFace,
    pub notification_filter: NotificationFilter,
    /// Turn the desktop's notification banners off for the length of each focus
    /// interval and put them back afterwards. Off by default: it changes a
    /// setting that belongs to the desktop, not to Pomodoro.
    pub silence_banners_during_focus: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            rounds_before_long_break: 4,
            auto_start_breaks: true,
            auto_start_focus: false,
            notifications: true,
            sound: true,
            theme: ThemePreference::System,
            timer_face: TimerFace::Digits,
            notification_filter: NotificationFilter::default(),
            silence_banners_during_focus: false,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.focus_minutes = self.focus_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.short_break_minutes = self.short_break_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.long_break_minutes = self.long_break_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.rounds_before_long_break = self.rounds_before_long_break.clamp(MIN_ROUNDS, MAX_ROUNDS);
        self.notification_filter = self.notification_filter.sanitized();
        self
    }

    pub fn duration_seconds(&self, phase: Phase) -> u32 {
        let minutes = match phase {
            Phase::Focus => self.focus_minutes,
            Phase::ShortBreak => self.short_break_minutes,
            Phase::LongBreak => self.long_break_minutes,
        };

        minutes.clamp(MIN_MINUTES, MAX_MINUTES).saturating_mul(60)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TimerState {
    pub phase: Phase,
    pub status: TimerStatus,
    pub duration_seconds: u32,
    pub remaining_seconds: u32,
    pub started_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub active_task_id: Option<String>,
    pub completed_in_cycle: u32,
}

impl Default for TimerState {
    fn default() -> Self {
        Self::for_phase(Phase::Focus, &Settings::default())
    }
}

impl TimerState {
    pub fn for_phase(phase: Phase, settings: &Settings) -> Self {
        let duration_seconds = settings.duration_seconds(phase);
        Self {
            phase,
            status: TimerStatus::Idle,
            duration_seconds,
            remaining_seconds: duration_seconds,
            started_at: None,
            ends_at: None,
            active_task_id: None,
            completed_in_cycle: 0,
        }
    }

    /// Returns the display value for a running timer without mutating it.
    ///
    /// Rounding up preserves the second currently visible to the user when the
    /// timer is paused between whole-second boundaries.
    pub fn current_remaining_seconds(&self, now_ms: i64) -> u32 {
        if self.status != TimerStatus::Running {
            return self.remaining_seconds.min(self.duration_seconds);
        }

        let Some(ends_at) = self.ends_at else {
            return self.remaining_seconds.min(self.duration_seconds);
        };
        let milliseconds_left = ends_at.saturating_sub(now_ms);
        if milliseconds_left <= 0 {
            return 0;
        }

        let seconds_left = milliseconds_left.saturating_add(999) / 1_000;
        u32::try_from(seconds_left)
            .unwrap_or(u32::MAX)
            .min(self.duration_seconds)
    }

    pub fn normalize_remaining(&mut self, now_ms: i64) {
        self.remaining_seconds = self.current_remaining_seconds(now_ms);
        if self.status != TimerStatus::Running {
            self.ends_at = None;
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FocusTask {
    pub id: String,
    pub title: String,
    pub estimate: u32,
    pub completed_pomodoros: u32,
    pub done: bool,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Interruption {
    pub id: String,
    pub text: String,
    pub category: InterruptionCategory,
    pub captured_at: i64,
    pub handled: bool,
    pub task_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub phase: Phase,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub duration_seconds: u32,
    pub started_at: i64,
    pub ended_at: i64,
    pub outcome: SessionOutcome,
}

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
        }
    }
}

impl AppData {
    /// Produces a display snapshot with a current countdown. Phase completion is
    /// deliberately handled by [`Self::tick`] so callers can notify and persist
    /// exactly once when a transition occurs.
    pub fn snapshot(&self, now_ms: i64) -> Self {
        let mut snapshot = self.clone();
        snapshot.timer.normalize_remaining(now_ms);
        snapshot
    }

    /// Starts an idle timer or resumes a paused timer.
    pub fn start_or_resume(&mut self, now_ms: i64) -> bool {
        if self.timer.status == TimerStatus::Running {
            self.timer.normalize_remaining(now_ms);
            return false;
        }

        if self.timer.remaining_seconds == 0 {
            self.timer.remaining_seconds = self.timer.duration_seconds;
        }
        if self.timer.status == TimerStatus::Idle {
            self.timer.started_at = Some(now_ms);
        } else if self.timer.started_at.is_none() {
            // Repair a partially persisted paused state without inventing
            // elapsed time.
            self.timer.started_at = Some(now_ms);
        }

        self.timer.status = TimerStatus::Running;
        self.timer.ends_at = Some(deadline_from(now_ms, self.timer.remaining_seconds));
        true
    }

    /// Pauses a running timer. If it is already due, completion wins over the
    /// pause request and is processed once.
    pub fn pause(&mut self, now_ms: i64) -> bool {
        if self.timer.status != TimerStatus::Running {
            return false;
        }
        if self.timer.current_remaining_seconds(now_ms) == 0 {
            self.tick(now_ms);
            return false;
        }

        self.timer.normalize_remaining(now_ms);
        self.timer.status = TimerStatus::Paused;
        self.timer.ends_at = None;
        true
    }

    /// Resets the current phase to its configured duration. An in-progress
    /// session is retained as abandoned for honest local statistics.
    pub fn reset(&mut self, now_ms: i64) {
        if self.timer.status != TimerStatus::Idle || self.timer.started_at.is_some() {
            self.record_session(SessionOutcome::Abandoned, now_ms);
        }
        let phase = self.timer.phase;
        self.enter_phase(phase, now_ms, false);
    }

    /// Skips the current phase without awarding focus credit. If the old phase
    /// was active, a skipped session is recorded.
    pub fn skip(&mut self, now_ms: i64) -> Phase {
        if self.timer.status == TimerStatus::Running
            && self.timer.current_remaining_seconds(now_ms) == 0
        {
            self.tick(now_ms);
            return self.timer.phase;
        }

        let skipped_phase = self.timer.phase;
        if self.timer.status != TimerStatus::Idle || self.timer.started_at.is_some() {
            self.record_session(SessionOutcome::Skipped, now_ms);
        }

        if skipped_phase == Phase::LongBreak {
            self.timer.completed_in_cycle = 0;
        }
        let next_phase = match skipped_phase {
            Phase::Focus => Phase::ShortBreak,
            Phase::ShortBreak | Phase::LongBreak => Phase::Focus,
        };
        let auto_start = self.should_auto_start(next_phase);
        self.enter_phase(next_phase, now_ms, auto_start);
        next_phase
    }

    /// Changes the phase explicitly. Active work is recorded as abandoned.
    pub fn set_phase(&mut self, phase: Phase, now_ms: i64) {
        if self.timer.phase == phase && self.timer.status == TimerStatus::Idle {
            return;
        }
        if self.timer.status != TimerStatus::Idle || self.timer.started_at.is_some() {
            self.record_session(SessionOutcome::Abandoned, now_ms);
        }
        self.enter_phase(phase, now_ms, false);
    }

    /// Advances an expired running phase at most once. The returned value is the
    /// phase that completed; `None` means no completion occurred.
    pub fn tick(&mut self, now_ms: i64) -> Option<Phase> {
        if self.timer.status != TimerStatus::Running {
            return None;
        }

        self.timer.normalize_remaining(now_ms);
        if self.timer.remaining_seconds > 0 {
            return None;
        }

        let completed_phase = self.timer.phase;
        self.record_session(SessionOutcome::Completed, now_ms);

        let next_phase = match completed_phase {
            Phase::Focus => {
                self.timer.completed_in_cycle = self.timer.completed_in_cycle.saturating_add(1);
                self.credit_active_task();
                if self.timer.completed_in_cycle >= self.settings.rounds_before_long_break {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak => Phase::Focus,
            Phase::LongBreak => {
                self.timer.completed_in_cycle = 0;
                Phase::Focus
            }
        };

        let auto_start = self.should_auto_start(next_phase);
        self.enter_phase(next_phase, now_ms, auto_start);
        Some(completed_phase)
    }

    pub fn select_task(&mut self, task_id: Option<String>) -> bool {
        match task_id {
            Some(task_id) => {
                if !self
                    .tasks
                    .iter()
                    .any(|task| task.id == task_id && !task.done)
                {
                    return false;
                }
                self.timer.active_task_id = Some(task_id);
            }
            None => self.timer.active_task_id = None,
        }
        true
    }

    pub fn create_task(
        &mut self,
        title: impl Into<String>,
        estimate: u32,
        now_ms: i64,
    ) -> Option<FocusTask> {
        let title = title.into().trim().to_owned();
        if title.is_empty() {
            return None;
        }
        let task = FocusTask {
            id: unique_id(
                "task",
                now_ms,
                self.tasks.iter().map(|task| task.id.as_str()),
            ),
            title,
            estimate: estimate.max(1),
            completed_pomodoros: 0,
            done: false,
            created_at: now_ms,
            completed_at: None,
        };
        self.tasks.push(task.clone());
        Some(task)
    }

    pub fn update_task(
        &mut self,
        id: &str,
        title: impl Into<String>,
        estimate: u32,
        done: bool,
        now_ms: i64,
    ) -> bool {
        let title = title.into().trim().to_owned();
        if title.is_empty() {
            return false;
        }
        let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) else {
            return false;
        };

        task.title = title;
        task.estimate = estimate.max(1);
        if task.done != done {
            task.done = done;
            task.completed_at = done.then_some(now_ms);
        } else if !done {
            task.completed_at = None;
        }
        if done
            && self.timer.active_task_id.as_deref() == Some(id)
            && (self.timer.phase != Phase::Focus || self.timer.status == TimerStatus::Idle)
        {
            self.timer.active_task_id = None;
        }
        true
    }

    pub fn set_task_done(&mut self, id: &str, done: bool, now_ms: i64) -> bool {
        let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        task.done = done;
        task.completed_at = done.then_some(now_ms);
        if done
            && self.timer.active_task_id.as_deref() == Some(id)
            && (self.timer.phase != Phase::Focus || self.timer.status == TimerStatus::Idle)
        {
            self.timer.active_task_id = None;
        }
        true
    }

    pub fn delete_task(&mut self, id: &str) -> bool {
        let old_len = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        if self.tasks.len() == old_len {
            return false;
        }
        if self.timer.active_task_id.as_deref() == Some(id) {
            self.timer.active_task_id = None;
        }
        for interruption in &mut self.interruptions {
            if interruption.task_id.as_deref() == Some(id) {
                interruption.task_id = None;
            }
        }
        for notification in &mut self.notifications {
            if notification.task_id.as_deref() == Some(id) {
                notification.task_id = None;
            }
        }
        true
    }

    pub fn capture_interruption(
        &mut self,
        text: impl Into<String>,
        category: InterruptionCategory,
        now_ms: i64,
    ) -> Option<Interruption> {
        let text = text.into().trim().to_owned();
        if text.is_empty() {
            return None;
        }
        let interruption = Interruption {
            id: unique_id(
                "interruption",
                now_ms,
                self.interruptions
                    .iter()
                    .map(|interruption| interruption.id.as_str()),
            ),
            text,
            category,
            captured_at: now_ms,
            handled: false,
            task_id: self.timer.active_task_id.clone(),
        };
        self.interruptions.push(interruption.clone());
        Some(interruption)
    }

    pub fn handle_interruption(&mut self, id: &str) -> bool {
        self.set_interruption_handled(id, true)
    }

    /// Turns a captured interruption into a task, at most once. Returns the task
    /// id whether it was created now or by an earlier call.
    pub fn convert_interruption_to_task(&mut self, id: &str, now_ms: i64) -> Option<String> {
        let (text, existing_task) = self
            .interruptions
            .iter()
            .find(|interruption| interruption.id == id)
            .map(|interruption| (interruption.text.clone(), interruption.task_id.clone()))?;

        if let Some(task_id) = existing_task {
            if self.tasks.iter().any(|task| task.id == task_id) {
                self.handle_interruption(id);
                return Some(task_id);
            }
        }

        let task = self.create_task(text, 1, now_ms)?;
        if let Some(interruption) = self
            .interruptions
            .iter_mut()
            .find(|interruption| interruption.id == id)
        {
            interruption.task_id = Some(task.id.clone());
            interruption.handled = true;
        }
        Some(task.id)
    }

    pub fn set_interruption_handled(&mut self, id: &str, handled: bool) -> bool {
        let Some(interruption) = self
            .interruptions
            .iter_mut()
            .find(|interruption| interruption.id == id)
        else {
            return false;
        };
        interruption.handled = handled;
        true
    }

    pub fn delete_interruption(&mut self, id: &str) -> bool {
        let old_len = self.interruptions.len();
        self.interruptions
            .retain(|interruption| interruption.id != id);
        self.interruptions.len() != old_len
    }

    /// Applies new settings immediately only when the timer is idle. Running and
    /// paused intervals keep the duration with which they began; the new values
    /// are used when a later phase is entered.
    pub fn update_settings(&mut self, settings: Settings) {
        self.settings = settings.sanitized();
        if self.timer.status == TimerStatus::Idle {
            let duration_seconds = self.settings.duration_seconds(self.timer.phase);
            self.timer.duration_seconds = duration_seconds;
            self.timer.remaining_seconds = duration_seconds;
            self.timer.started_at = None;
            self.timer.ends_at = None;
        }
    }

    /// True while a focus interval is actually counting down.
    ///
    /// The status alone is not enough. A focus interval that ran out keeps
    /// `Running` until the next [`Self::tick`], which can be up to half a second
    /// later; anything arriving in that gap is not during focus and must not be
    /// filed as if it were. Asking for the remaining time at `now_ms` closes the
    /// gap without waiting for the tick.
    pub fn is_focus_running(&self, now_ms: i64) -> bool {
        self.timer.phase == Phase::Focus
            && self.timer.status == TimerStatus::Running
            && self.timer.current_remaining_seconds(now_ms) > 0
    }

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
            if let Some(position) = self.notifications.iter().position(|notification| {
                notification.replaces_id == replaces_id
                    && notification.app_name.to_lowercase() == app_name.to_lowercase()
            }) {
                let mut existing = self.notifications.remove(position);
                // Only genuinely new words are worth re-reading. An update that
                // repeats the same text leaves a triaged row triaged.
                if existing.summary != summary || existing.body != body {
                    existing.triaged = false;
                }
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

    fn record_session(&mut self, outcome: SessionOutcome, now_ms: i64) {
        let phase = self.timer.phase;
        let started_at = self.timer.started_at.unwrap_or(now_ms).min(now_ms);
        let remaining_seconds = self.timer.current_remaining_seconds(now_ms);
        let duration_seconds = if outcome == SessionOutcome::Completed {
            self.timer.duration_seconds
        } else {
            self.timer
                .duration_seconds
                .saturating_sub(remaining_seconds)
        };
        let task_id = self.timer.active_task_id.clone();
        let task_title = task_id.as_deref().and_then(|id| {
            self.tasks
                .iter()
                .find(|task| task.id == id)
                .map(|task| task.title.clone())
        });
        let id = unique_id(
            "session",
            now_ms,
            self.sessions.iter().map(|session| session.id.as_str()),
        );

        self.sessions.push(SessionRecord {
            id,
            phase,
            task_id,
            task_title,
            duration_seconds,
            started_at,
            ended_at: now_ms,
            outcome,
        });
    }

    fn credit_active_task(&mut self) {
        let Some(active_task_id) = self.timer.active_task_id.as_deref() else {
            return;
        };
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == active_task_id) {
            task.completed_pomodoros = task.completed_pomodoros.saturating_add(1);
        }
    }

    fn should_auto_start(&self, phase: Phase) -> bool {
        if phase.is_break() {
            self.settings.auto_start_breaks
        } else {
            self.settings.auto_start_focus
                && self
                    .timer
                    .active_task_id
                    .as_deref()
                    .is_some_and(|id| self.tasks.iter().any(|task| task.id == id && !task.done))
        }
    }

    fn enter_phase(&mut self, phase: Phase, now_ms: i64, start: bool) {
        if phase == Phase::Focus
            && !self
                .timer
                .active_task_id
                .as_deref()
                .is_some_and(|id| self.tasks.iter().any(|task| task.id == id && !task.done))
        {
            self.timer.active_task_id = None;
        }
        let duration_seconds = self.settings.duration_seconds(phase);
        self.timer.phase = phase;
        self.timer.duration_seconds = duration_seconds;
        self.timer.remaining_seconds = duration_seconds;
        self.timer.started_at = None;
        self.timer.ends_at = None;
        self.timer.status = TimerStatus::Idle;

        if start {
            self.timer.status = TimerStatus::Running;
            self.timer.started_at = Some(now_ms);
            self.timer.ends_at = Some(deadline_from(now_ms, duration_seconds));
        }
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
    let existing: Vec<&str> = existing.collect();
    for counter in 0_u32.. {
        let candidate = format!("notif-{now_ms}-{counter}");
        if !existing.iter().any(|id| *id == candidate) {
            return candidate;
        }
    }
    unreachable!("the finite set of existing IDs cannot exhaust all u32 counters")
}

fn deadline_from(now_ms: i64, seconds: u32) -> i64 {
    now_ms.saturating_add(i64::from(seconds).saturating_mul(1_000))
}

fn unique_id<'a>(prefix: &str, now_ms: i64, existing: impl Iterator<Item = &'a str>) -> String {
    let existing: Vec<&str> = existing.collect();
    let base = format!("{prefix}-{now_ms}");
    if !existing.iter().any(|candidate| *candidate == base) {
        return base;
    }

    for suffix in 1_u32.. {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|existing_id| *existing_id == candidate) {
            return candidate;
        }
    }
    unreachable!("the finite set of existing IDs cannot exhaust all u32 suffixes")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn serde_uses_the_typescript_camel_case_contract() {
        let value = serde_json::to_value(AppData::default()).unwrap();
        assert_eq!(value["timer"]["phase"], "focus");
        assert_eq!(value["timer"]["status"], "idle");
        assert_eq!(value["timer"]["durationSeconds"], 1_500);
        assert_eq!(value["settings"]["shortBreakMinutes"], 5);
        assert_eq!(value["settings"]["theme"], "system");
    }

    #[test]
    fn task_and_interruption_crud_maintain_references() {
        let mut data = AppData::default();
        let task = data.create_task("  Read paper  ", 0, 7).unwrap();
        assert_eq!(task.title, "Read paper");
        assert_eq!(task.estimate, 1);
        assert!(data.select_task(Some(task.id.clone())));

        let interruption = data
            .capture_interruption("  Check email  ", InterruptionCategory::External, 8)
            .unwrap();
        assert_eq!(interruption.task_id.as_deref(), Some(task.id.as_str()));
        assert!(data.handle_interruption(&interruption.id));
        assert!(data.interruptions[0].handled);

        assert!(data.delete_task(&task.id));
        assert_eq!(data.timer.active_task_id, None);
        assert_eq!(data.interruptions[0].task_id, None);
        assert!(data.delete_interruption(&interruption.id));
        assert!(data.interruptions.is_empty());
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
    fn turning_an_interruption_into_a_task_twice_makes_one_task() {
        let mut data = AppData::default();
        let captured = data
            .capture_interruption("Check the deploy", InterruptionCategory::Internal, 10)
            .unwrap();

        let first = data
            .convert_interruption_to_task(&captured.id, 20)
            .expect("the first conversion creates a task");
        let second = data
            .convert_interruption_to_task(&captured.id, 30)
            .expect("the second returns the task the first made");

        assert_eq!(first, second);
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(
            data.interruptions[0].task_id.as_deref(),
            Some(first.as_str())
        );
        assert!(data.interruptions[0].handled);

        assert!(data
            .convert_interruption_to_task("interruption-missing", 40)
            .is_none());
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
}
