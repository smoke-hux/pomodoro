//! Tasks and manual interruptions, including attribution and conversion links.

use super::{unique_id, AppData, Phase, TimerStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InterruptionCategory {
    #[default]
    Internal,
    External,
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
    /// The task that was active when this was captured: "noted while working
    /// on X". It says nothing about conversion, and is never read as if it did.
    pub task_id: Option<String>,
    /// The task this interruption was turned into, if any.
    ///
    /// Separate from `task_id` because one field used to do both jobs. An
    /// interruption captured mid-focus — the normal case — already carried the
    /// active task's id, "Turn into task" took that for the mark of an earlier
    /// conversion, ticked the note as handled and made nothing, under a
    /// message saying the task had been added. Missing from stores written
    /// before it existed, where it loads as `None`.
    pub converted_task_id: Option<String>,
}

impl AppData {
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
            if interruption.converted_task_id.as_deref() == Some(id) {
                interruption.converted_task_id = None;
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
            converted_task_id: None,
        };
        self.interruptions.push(interruption.clone());
        Some(interruption)
    }

    pub fn handle_interruption(&mut self, id: &str) -> bool {
        self.set_interruption_handled(id, true)
    }

    /// Turns a captured interruption into a task, at most once. Returns the task
    /// id whether it was created now or by an earlier call.
    ///
    /// "An earlier call" is judged by `converted_task_id` alone. The task the
    /// note was captured under, `task_id`, is somebody else's task and proves
    /// nothing; see [`Interruption::converted_task_id`] for what reading it
    /// here used to cost.
    pub fn convert_interruption_to_task(&mut self, id: &str, now_ms: i64) -> Option<String> {
        let (text, existing_task) = self
            .interruptions
            .iter()
            .find(|interruption| interruption.id == id)
            .map(|interruption| {
                (
                    interruption.text.clone(),
                    interruption.converted_task_id.clone(),
                )
            })?;

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
            interruption.converted_task_id = Some(task.id.clone());
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
}
