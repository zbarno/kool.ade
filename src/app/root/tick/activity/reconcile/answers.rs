//! Repair stale parent cards from durable, successfully applied child replies.
use crate::{
    app::session::Project,
    core::planning_work::{Work, WorkStatus},
};

pub(super) fn reconcile(project: &Project, work: &mut [Work], active_key: Option<&str>) -> bool {
    let excluded = work
        .iter()
        .filter(|parent| {
            parent.status != WorkStatus::NeedsAttention
                || Some(parent.key.as_str()) == active_key
                || project.task_turns.contains_key(&parent.key)
                || project.task_chats.has_failed_task_reply(&parent.key)
                || project.task_chats.has_pending_user_reply(&parent.key)
                || project.cancelled_work.contains(
                    &crate::persistence::cancelled_work::planning_id(&parent.uid),
                )
        })
        .map(|parent| parent.key.clone())
        .collect();
    let mut changed = false;
    for item in &project.state.resolved_items {
        let key = item.conversation_key();
        let Some(messages) = project.task_chats.messages.get(key) else {
            continue;
        };
        let Some(user) = messages.iter().rposition(|message| {
            message.role == crate::domain::ChatRole::User && !message.clarification
        }) else {
            continue;
        };
        let applied = messages[user + 1..].iter().any(|message| {
            message.role == crate::domain::ChatRole::System
                && (message
                    .id
                    .starts_with("internal-task-reply-outcome:applied:")
                    || message
                        .id
                        .starts_with("internal-retry:planning-write-applied:"))
        });
        if !applied
            || project.task_chats.has_failed_task_reply(key)
            || project.task_chats.has_pending_user_reply(key)
        {
            continue;
        }
        changed |= crate::core::planning_work::reconcile_answered_child(
            &project.state,
            &project.state,
            key,
            work,
            &excluded,
        );
    }
    changed
}
