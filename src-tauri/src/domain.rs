use serde::{Deserialize, Serialize};

const MIN_MINUTES: u32 = 1;
const MAX_MINUTES: u32 = 24 * 60;
const MIN_ROUNDS: u32 = 1;
const MAX_ROUNDS: u32 = 24;

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InterruptionCategory {
    #[default]
    Internal,
    External,
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
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.focus_minutes = self.focus_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.short_break_minutes = self.short_break_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.long_break_minutes = self.long_break_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        self.rounds_before_long_break = self.rounds_before_long_break.clamp(MIN_ROUNDS, MAX_ROUNDS);
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
}
