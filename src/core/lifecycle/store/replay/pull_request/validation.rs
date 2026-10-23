use super::super::super::task_transition::PullRequestPreparationStatus;
use super::super::super::{
    LifecycleEventKind, LifecycleEventRecord, PullRequestPreparation, TaskRecord,
};
use crate::artifacts::planning_store::StoreError;

mod historical_authorization;
pub(super) use historical_authorization::authorization_matches_historical_scope;

pub(super) fn same_pr_identity(
    left: &PullRequestPreparation,
    right: &PullRequestPreparation,
) -> bool {
    let refreshed_scope = left.status == PullRequestPreparationStatus::NeedsAttention
        && right.status == PullRequestPreparationStatus::Publishing
        && left.publication_scope.is_some()
        && right.publication_scope.is_some()
        && left.publication_scope != right.publication_scope
        && left.publication_authorization != right.publication_authorization;
    left.uid == right.uid
        && left.work_item_uid == right.work_item_uid
        && left.feature_uid == right.feature_uid
        && left.task_uid == right.task_uid
        && left.included_task_uids == right.included_task_uids
        && (left.publication_scope == right.publication_scope
            || refreshed_scope
            || (left.publication_scope.is_none()
                && right.publication_scope.is_some()
                && right.status == PullRequestPreparationStatus::Publishing))
        && left.created_at_ms == right.created_at_ms
        && if left.publication_authorization.is_none() {
            right.publication_authorization.is_none()
                || right.status == PullRequestPreparationStatus::Publishing
        } else {
            left.publication_authorization == right.publication_authorization || refreshed_scope
        }
}

pub(super) fn valid_authorization_event_uids(
    left: &PullRequestPreparation,
    right: &PullRequestPreparation,
    event: &super::super::super::LifecycleEventRecord,
) -> bool {
    let mut unique = std::collections::HashSet::new();
    if right.publication_authorization_event_uids.len() > 64
        || right
            .publication_authorization_event_uids
            .iter()
            .any(|uid| uuid::Uuid::parse_str(uid).is_err() || !unique.insert(uid))
        || left
            .publication_authorization_event_uids
            .iter()
            .any(|uid| !right.publication_authorization_event_uids.contains(uid))
    {
        return false;
    }
    let added = right
        .publication_authorization_event_uids
        .iter()
        .filter(|uid| !left.publication_authorization_event_uids.contains(uid))
        .collect::<Vec<_>>();
    let refreshed_scope = left.status == PullRequestPreparationStatus::NeedsAttention
        && right.status == PullRequestPreparationStatus::Publishing
        && left.publication_scope != right.publication_scope;
    added.is_empty()
        || (added.len() == 1
            && added[0] == &event.uid
            && right.status == PullRequestPreparationStatus::Publishing
            && left.status != PullRequestPreparationStatus::Publishing
            && right.publication_authorization.is_some()
            && (if refreshed_scope {
                event.actor == super::super::super::LifecycleActor::User
            } else {
                matches!(
                    event.actor,
                    super::super::super::LifecycleActor::User
                        | super::super::super::LifecycleActor::ApplicationPolicy
                )
            }))
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
            PullRequestPreparationStatus::AwaitingAuthorization,
            PullRequestPreparationStatus::NeedsAttention
        ) | (
            PullRequestPreparationStatus::NeedsAttention,
            PullRequestPreparationStatus::NeedsAttention
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::Opened
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::ClosedUnmerged
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

pub(super) fn valid_recovery_transition(
    left: &PullRequestPreparation,
    right: &PullRequestPreparation,
) -> bool {
    let Some(next) = right.recovery.as_ref() else {
        return left.recovery.is_none(); // Historical records remain replayable.
    };
    let previous_attempt = left
        .recovery
        .as_ref()
        .map(|recovery| recovery.attempt)
        .unwrap_or_else(|| u64::from(left.publication_authorization.is_some()));
    let begins_attempt = right.status == PullRequestPreparationStatus::Publishing
        && left.status != PullRequestPreparationStatus::Publishing;
    let expected_attempt = previous_attempt.checked_add(u64::from(begins_attempt));
    expected_attempt == Some(next.attempt)
        && right.publication_lease_epoch()
            == left
                .publication_lease_epoch()
                .saturating_add(u64::from(begins_attempt))
        && if right.status == PullRequestPreparationStatus::NeedsAttention {
            next.last_failure.is_some()
                || left
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.last_failure.is_none())
        } else {
            left.recovery
                .as_ref()
                .and_then(|recovery| recovery.last_failure.as_ref())
                == next.last_failure.as_ref()
        }
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
        || (!matches!(task.phase, crate::core::lifecycle::TaskPhase::Done)
            && (!matches!(
                task.phase,
                crate::core::lifecycle::TaskPhase::ReReview
                    | crate::core::lifecycle::TaskPhase::AwaitingReview
                    | crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
                    | crate::core::lifecycle::TaskPhase::AwaitingSpecReconciliation
            ) || !matches!(
                preparation.status,
                PullRequestPreparationStatus::AwaitingAuthorization
                    | PullRequestPreparationStatus::NeedsAttention
            ) || preparation.provider_pr_url.is_some()))
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
    let current_scope = preparation.publication_scope.as_ref().is_none_or(|scope| {
        scope.source_sha == task.reviewed_code_head_sha.as_deref().unwrap_or_default()
            && scope.reviewed_diff_hash == task.implementation_sha256.as_deref().unwrap_or_default()
            && scope.task_evidence.iter().any(|evidence| {
                evidence.task_uid == task.task_uid
                    && evidence.task_revision == task.revision
                    && Some(evidence.reviewed_head_sha.as_str())
                        == task.reviewed_code_head_sha.as_deref()
                    && Some(evidence.reviewed_diff_hash.as_str())
                        == task.implementation_sha256.as_deref()
                    && task.code_review.as_ref() == Some(&evidence.code_review_receipt)
                    && task.runtime_qa.as_ref() == Some(&evidence.qa_receipt)
                    && task.spec_reconciliation.as_ref() == Some(&evidence.spec_sync_receipt)
            })
    });
    task.phase == crate::core::lifecycle::TaskPhase::Done
        && current_scope
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
