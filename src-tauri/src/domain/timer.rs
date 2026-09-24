//! Deadline-based countdown, phase transitions, and session accounting.

use super::{unique_id, AppData, Settings};
use serde::{Deserialize, Serialize};

/// Oldest-first cap on session history. Roughly two years at six intervals a
/// day; beyond that the oldest records are dropped on write rather than the
/// store growing forever.
pub const SESSION_RETENTION: usize = 5_000;

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

impl AppData {
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

    /// Pauses for a quit. A phase that is already due is completed, as [`Self::pause`]
    /// would, except that the next phase waits, idle, instead of starting
    /// itself: nobody is there to take it. A break started here was saved as
    /// running, and the next launch recorded it as taken.
    pub fn pause_to_quit(&mut self, now_ms: i64) {
        if self.complete_due(now_ms, false).is_none() {
            self.pause(now_ms);
        }
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
        self.complete_due(now_ms, true)
    }

    /// Settles, at launch, a phase whose deadline passed while the app was not
    /// running — a crash or a kill, since an orderly quit pauses the timer
    /// first. Returns whether anything changed, so the caller can save.
    ///
    /// The first [`Self::tick`] after launch used to do this, and treated the
    /// deadline as having just gone by: the next phase started itself, so the
    /// app opened the morning after a crash already counting down a break
    /// nobody was taking, and the caller announced "Focus complete" for an
    /// interval that had ended the evening before. The work was still done, so
    /// the session is recorded at its deadline, the task credited and the
    /// cycle moved on exactly as `tick` would; but the next phase waits, idle,
    /// to be started, and the caller is expected to say nothing.
    ///
    /// A running phase that is not yet due is left alone. It is still
    /// legitimately counting: the deadline is wall-clock time, and it did not
    /// stop because the process did.
    pub fn recover_at_launch(&mut self, now_ms: i64) -> bool {
        self.complete_due(now_ms, false).is_some()
    }

    /// The completion behind [`Self::tick`] and [`Self::recover_at_launch`],
    /// which differ only in whether the next phase may start itself.
    fn complete_due(&mut self, now_ms: i64, allow_auto_start: bool) -> Option<Phase> {
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

        let auto_start = allow_auto_start && self.should_auto_start(next_phase);
        self.enter_phase(next_phase, now_ms, auto_start);
        Some(completed_phase)
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

    fn record_session(&mut self, outcome: SessionOutcome, now_ms: i64) {
        let phase = self.timer.phase;
        // A completion is noticed, not witnessed: after a suspend, or a launch
        // following a crash, `now_ms` can be hours past the moment the phase
        // ran out, and stamping it with `now_ms` put yesterday evening's focus
        // into this morning's ledger. The deadline is when it ended. Never
        // later than now, so a clock that moved backwards cannot file a
        // session in the future. Anything the user ended by hand ended now.
        let ended_at = if outcome == SessionOutcome::Completed {
            self.timer.ends_at.unwrap_or(now_ms).min(now_ms)
        } else {
            now_ms
        };
        let started_at = self.timer.started_at.unwrap_or(ended_at).min(ended_at);
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
            ended_at,
            outcome,
        });
        if self.sessions.len() > SESSION_RETENTION {
            let excess = self.sessions.len() - SESSION_RETENTION;
            self.sessions.drain(..excess);
        }
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
