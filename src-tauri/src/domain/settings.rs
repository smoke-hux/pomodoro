//! Preferences, validation bounds, and application to idle intervals.

use super::{AppData, NotificationFilter, Phase, TimerStatus};
use serde::{Deserialize, Serialize};

const MIN_MINUTES: u32 = 1;
const MAX_MINUTES: u32 = 24 * 60;
const MIN_ROUNDS: u32 = 1;
const MAX_ROUNDS: u32 = 24;

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

impl AppData {
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
}
