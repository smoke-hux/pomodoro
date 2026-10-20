//! Desktop banner wording and the independent task alert preferences.

use crate::domain::{AppData, Phase};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Announces the end of a phase with a banner. The banner is silent, with no
/// sound hint: the sound is [`crate::sound::play`]'s business alone, see there.
pub(crate) fn notify_boundary(app: &AppHandle, completed: Phase, snapshot: &AppData) {
    if !snapshot.settings.notifications {
        return;
    }

    let (title, body) = boundary_banner(completed, snapshot);
    let _ = app.notification().builder().title(title).body(body).show();
}

/// The title and text of the banner for the end of `completed`. The count of
/// rounds is the one actually finished: it used to say four whatever the
/// setting was.
fn boundary_banner(completed: Phase, snapshot: &AppData) -> (&'static str, String) {
    match completed {
        Phase::Focus => match snapshot.timer.phase {
            Phase::LongBreak => {
                let rounds = match snapshot.timer.completed_in_cycle {
                    1 => "a round".to_string(),
                    count => format!("{count} rounds"),
                };
                (
                    "Focus cycle complete",
                    format!("You finished {rounds}. Take a restorative long break."),
                )
            }
            _ => ("Focus complete", "Step away for a short break.".to_string()),
        },
        Phase::ShortBreak | Phase::LongBreak => (
            "Break complete",
            "Choose one task when you are ready to focus again.".to_string(),
        ),
    }
}

/// What toggling a task should set off besides the change itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskAlert {
    pub(crate) sound: bool,
    pub(crate) notify: bool,
}

/// Decides [`TaskAlert`] for a task that was `was_done` and is `now_done`.
///
/// Only finishing a task is an occasion. Reopening one is a correction, often
/// of a mis-click a second earlier, and congratulating it would be wrong
/// twice. Each alert then follows its own setting and nothing else. Kept free
/// of the state because the command around it cannot run without a window.
pub(crate) fn task_alert(
    was_done: bool,
    now_done: bool,
    sound: bool,
    notifications: bool,
) -> TaskAlert {
    let finished = !was_done && now_done;
    TaskAlert {
        sound: finished && sound,
        notify: finished && notifications,
    }
}

/// The one line a "task complete" notification carries: the task's title,
/// cut short if it would not fit a banner, and never empty.
pub(crate) fn task_complete_summary(title: &str) -> String {
    const MOST: usize = 80;
    let title = title.trim();
    let shown: String = if title.chars().count() > MOST {
        format!("{}…", title.chars().take(MOST - 1).collect::<String>())
    } else {
        title.to_string()
    };
    if shown.is_empty() {
        "Task complete".to_string()
    } else {
        format!("Task complete: {shown}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_long_break(rounds: u32) -> AppData {
        let mut data = AppData::default();
        data.timer.phase = Phase::LongBreak;
        data.timer.completed_in_cycle = rounds;
        data
    }

    #[test]
    fn the_long_break_banner_counts_the_rounds_actually_finished() {
        assert_eq!(
            boundary_banner(Phase::Focus, &at_long_break(2)).1,
            "You finished 2 rounds. Take a restorative long break."
        );
        assert_eq!(
            boundary_banner(Phase::Focus, &at_long_break(1)).1,
            "You finished a round. Take a restorative long break."
        );
        assert_eq!(
            boundary_banner(Phase::Focus, &AppData::default()),
            ("Focus complete", "Step away for a short break.".to_string())
        );
    }
    #[test]
    fn a_task_complete_notification_names_the_task_in_its_summary() {
        assert_eq!(
            task_complete_summary("  Fix a<b && c>d  "),
            "Task complete: Fix a<b && c>d"
        );
        assert_eq!(task_complete_summary("   "), "Task complete");
        let long = "x".repeat(200);
        let summary = task_complete_summary(&long);
        assert!(summary.ends_with('…'));
        assert!(summary.chars().count() <= "Task complete: ".len() + 80);
    }

    #[test]
    fn finishing_a_task_alerts_and_reopening_one_does_not() {
        let alert = |sound, notify| TaskAlert { sound, notify };
        // Finished: each alert follows its own setting, independently.
        assert_eq!(task_alert(false, true, true, true), alert(true, true));
        assert_eq!(task_alert(false, true, true, false), alert(true, false));
        assert_eq!(task_alert(false, true, false, true), alert(false, true));
        assert_eq!(task_alert(false, true, false, false), alert(false, false));
        // Reopened, or not changed at all: nothing, whatever the settings.
        assert_eq!(task_alert(true, false, true, true), alert(false, false));
        assert_eq!(task_alert(true, true, true, true), alert(false, false));
        assert_eq!(task_alert(false, false, true, true), alert(false, false));
    }
}
