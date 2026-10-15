use super::super::super::task_transition::PullRequestPreparationStatus;
use super::super::super::{
    LifecycleEventKind, LifecycleEventRecord, PullRequestPreparation, TaskRecord,
};
use crate::artifacts::planning_store::StoreError;

pub(super) fn same_pr_identity(
    left: &PullRequestPreparation,
    right: &PullRequestPreparation,
) -> bool {
    left.uid == right.uid
        && left.work_item_uid == right.work_item_uid
        && left.feature_uid == right.feature_uid
        && left.task_uid == right.task_uid
        && left.included_task_uids == right.included_task_uids
        && left.created_at_ms == right.created_at_ms
        && if left.status == PullRequestPreparationStatus::AwaitingAuthorization {
            right.publication_authorization.is_some()
        } else {
            left.publication_authorization == right.publication_authorization
        }
}

pub(super) fn allowed_transition(
    from: PullRequestPreparationStatus,
    to: PullRequestPreparationStatus,
) -> bool {
    matches!(
        (from, to),
        (
            PullRequestPreparationStatus::AwaitingAuthorization,
            PullRequestPreparationStatus::Publishing
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::Opened
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::NeedsAttention
        ) | (
            PullRequestPreparationStatus::NeedsAttention,
            PullRequestPreparationStatus::Publishing
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::Opened
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::ClosedUnmerged
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::Merged
        ) | (
            PullRequestPreparationStatus::ClosedUnmerged,
            PullRequestPreparationStatus::Opened
        )
    )
}

pub(super) fn ensure_pr_task_binding(
    preparation: &PullRequestPreparation,
    task: &TaskRecord,
    work_item_uid: &str,
    events: &[LifecycleEventRecord],
    tasks: &[TaskRecord],
    completed_at_ms: u64,
) -> Result<(), StoreError> {
    if preparation.work_item_uid != work_item_uid
        || preparation.uid != work_item_uid
        || preparation.task_uid != task.task_uid
        || preparation.feature_uid != task.feature_uid
        || task.pr_work_item_uid.as_deref() != Some(work_item_uid)
        || task.phase != crate::core::lifecycle::TaskPhase::Done
        || !pr_scope_matches_task(preparation, task, events, tasks, completed_at_ms)
    {
        return Err(StoreError::MalformedState(
            "PR preparation identity or task scope does not match its completed task".into(),
        ));
    }
    Ok(())
}

pub(super) fn authorization_matches_task(
    preparation: &PullRequestPreparation,
    task: &TaskRecord,
    work_item_uid: &str,
) -> bool {
    let Some(receipt) = preparation.publication_authorization.as_ref() else {
        return false;
    };
    task.phase == crate::core::lifecycle::TaskPhase::Done
        && receipt.validate().is_ok()
        && receipt.actor == crate::core::lifecycle::GateActor::User
        && receipt.decision == crate::core::lifecycle::GateDecision::Approved
        && receipt.subject_uid == work_item_uid
        && Some(receipt.subject_sha256.as_str()) == task.implementation_sha256.as_deref()
        && receipt.evidence_refs == [format!("user-action:create-pr:{work_item_uid}")]
}

fn pr_scope_matches_task(
    preparation: &PullRequestPreparation,
    task: &TaskRecord,
    events: &[LifecycleEventRecord],
    tasks: &[TaskRecord],
    completed_at_ms: u64,
) -> bool {
    preparation.included_task_uids.len() == task.subtask_uids.len() + 1
        && preparation.included_task_uids.contains(&task.task_uid)
        && task.subtask_uids.iter().all(|uid| {
            preparation.included_task_uids.contains(uid)
                && tasks.iter().any(|child| {
                    child.task_uid == *uid
                        && child.feature_uid == task.feature_uid
                        && child.parent_task_uid.as_deref() == Some(task.task_uid.as_str())
                        && child.phase == crate::core::lifecycle::TaskPhase::Done
                        && events.iter().any(|event| {
                            event.task_uid.as_deref() == Some(uid.as_str())
                                && event.updated_at_ms <= completed_at_ms
                                && matches!(
                                    &event.event,
                                    LifecycleEventKind::Task(
                                        crate::core::lifecycle::TaskEvent::SubtaskReconciled { .. }
                                    )
                                )
                        })
                })
        })
}
