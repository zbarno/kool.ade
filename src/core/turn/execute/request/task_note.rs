use crate::core::state::PlannerState;
use crate::core::{planning_work, prompt, task_conversation};

pub(super) fn for_target(task: Option<&str>, state: &PlannerState) -> Option<&'static str> {
    let key = task?;
    if task_conversation::is_opportunity_conversation_key(key) {
        return Some(prompt::OPPORTUNITY_TASK_MODE_NOTE);
    }
    if key.starts_with("planning:") || key.starts_with("feature:") {
        return None;
    }
    if let Some(work) = planning_work::find(state, key) {
        return (work.kind == planning_work::WorkKind::Question)
            .then_some(prompt::QUESTION_TASK_MODE_NOTE);
    }
    Some(prompt::TASK_CONVERSATION_MODE_NOTE)
}
