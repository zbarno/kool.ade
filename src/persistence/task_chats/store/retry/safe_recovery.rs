//! Only failures known to occur before application may be replayed automatically.
use super::*;
const PREFIX: &str = "internal-task-reply-recoverable:";

pub(super) fn is_recoverable(message: &ChatMessage) -> bool {
    message.role == ChatRole::System
        && (message.id.starts_with(PREFIX)
            || (message.id.starts_with(TASK_REPLY_FAILED_PREFIX)
                && message
                    .text
                    .strip_prefix("⚠ Turn rejected — nothing was written.\n")
                    .is_some_and(|problems| {
                        !problems.is_empty() && problems.lines().all(format_problem)
                    })))
}
pub(super) fn attempts(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .filter(|message| is_recoverable(message))
        .count()
}
pub(crate) fn format_problem(problem: &str) -> bool {
    problem.starts_with("Structured JSON block is malformed")
        || problem.starts_with(
            "The planner's final message did not include the required structured JSON block",
        )
        || (problem.starts_with('“')
            && problem.ends_with("cannot update and resolve the same item in one turn"))
}
impl TaskChats {
    /// One durable internal retry per submitted answer; subsequent failures stay visible.
    pub fn mark_recoverable_task_reply(message: &mut ChatMessage) {
        message.id = format!("{PREFIX}{}", uuid::Uuid::new_v4());
        message.text.push_str("\nYour answer is saved. Kool.ad/e will attempt one internal recovery; do not submit the same answer again. Repair only the response format reported above, preserve the submitted choice and original scope, and return the full validated envelope. For an item being resolved, omit its redundant open_items_updated entry. Never bypass authorization or feature identity checks.");
    }
}
