use crate::core::lifecycle::{
    GateActor, GateDecision, LifecycleActor, LifecycleEventKind, LifecycleEventRecord,
    PullRequestPreparation, TaskEvent,
};

pub(in crate::core::lifecycle::store::replay::pull_request) fn authorization_matches_historical_scope(
    preparation: &PullRequestPreparation,
    task_uid: &str,
    work_item_uid: &str,
    authorization_event: &LifecycleEventRecord,
    events: &[LifecycleEventRecord],
) -> bool {
    let (Some(authorization), Some(scope)) = (
        preparation.publication_authorization.as_ref(),
        preparation.publication_scope.as_ref(),
    ) else {
        return false;
    };
    if authorization_event.actor != LifecycleActor::User
        || authorization_event.task_uid.as_deref() != Some(task_uid)
        || !matches!(
            &authorization_event.event,
            LifecycleEventKind::PullRequestState { work_item_uid: event_work_uid, status }
                if event_work_uid == work_item_uid
                    && *status == crate::core::lifecycle::PullRequestPreparationStatus::Publishing
        )
        || authorization_event.pr_preparation_after.as_ref() != Some(preparation)
        || authorization.actor != GateActor::User
        || authorization.decision != GateDecision::Approved
        || authorization.subject_uid != work_item_uid
        || authorization.subject_sha256 != scope.reviewed_diff_hash
        || authorization.evidence_refs != [format!("user-action:create-pr:{work_item_uid}")]
        || scope.validate(&preparation.included_task_uids).is_err()
    {
        return false;
    }

    let Some(parent) = scope
        .task_evidence
        .iter()
        .find(|item| item.task_uid == task_uid)
    else {
        return false;
    };
    if parent.reviewed_diff_hash != scope.reviewed_diff_hash {
        return false;
    }
    scope.task_evidence.iter().all(|evidence| {
        let review_seen = events.iter().any(|event| {
            event.task_uid.as_deref() == Some(evidence.task_uid.as_str())
                && event.feature_uid == preparation.feature_uid
                && event.updated_at_ms <= authorization_event.updated_at_ms
                && event
                    .task_revision_after
                    .is_some_and(|revision| revision <= evidence.task_revision)
                && event.actor == LifecycleActor::IndependentReviewer
                && matches!(
                    &event.event,
                    LifecycleEventKind::Task(TaskEvent::CodeReview {
                        receipt,
                        reviewed_head_sha,
                        ..
                    }) if receipt == &evidence.code_review_receipt
                        && reviewed_head_sha == &evidence.reviewed_head_sha
                        && receipt.subject_sha256 == evidence.reviewed_diff_hash
                )
        });
        let qa_seen = events.iter().any(|event| {
            let current_task_event = event.task_uid.as_deref() == Some(evidence.task_uid.as_str())
                && event.feature_uid == preparation.feature_uid
                && event.updated_at_ms <= authorization_event.updated_at_ms
                && event
                    .task_revision_after
                    .is_some_and(|revision| revision <= evidence.task_revision);
            current_task_event
                && (matches!(
                    &event.event,
                    LifecycleEventKind::Task(TaskEvent::RuntimeQaCompleted { receipt, .. })
                        if receipt == &evidence.qa_receipt
                            && receipt.subject_sha256 == evidence.reviewed_diff_hash
                            && event.actor == LifecycleActor::IndependentReviewer
                ) || matches!(
                    &event.event,
                    LifecycleEventKind::Task(TaskEvent::RuntimeQaNotApplicable { receipt, .. })
                        if receipt.as_ref() == &evidence.qa_receipt
                            && receipt.subject_sha256 == evidence.reviewed_diff_hash
                            && event.actor == LifecycleActor::Manager
                ))
        });
        let spec_seen = events.iter().any(|event| {
            event.task_uid.as_deref() == Some(evidence.task_uid.as_str())
                && event.feature_uid == preparation.feature_uid
                && event.updated_at_ms <= authorization_event.updated_at_ms
                && event.task_revision_after == Some(evidence.task_revision)
                && event.actor == LifecycleActor::ApplicationPolicy
                && if evidence.task_uid == task_uid {
                    matches!(
                        &event.event,
                        LifecycleEventKind::Task(TaskEvent::ReconcileSpec {
                            receipt,
                            pr_work_item_uid,
                        }) if receipt == &evidence.spec_sync_receipt
                            && pr_work_item_uid == work_item_uid
                            && receipt.subject_sha256 == evidence.reviewed_diff_hash
                    ) || matches!(
                        &event.event,
                        LifecycleEventKind::Task(TaskEvent::PublicationGatesRefreshed { receipt })
                            if receipt == &evidence.spec_sync_receipt
                                && receipt.subject_sha256 == evidence.reviewed_diff_hash
                    )
                } else {
                    matches!(
                        &event.event,
                        LifecycleEventKind::Task(TaskEvent::SubtaskReconciled { receipt })
                            if receipt == &evidence.spec_sync_receipt
                                && receipt.subject_sha256 == evidence.reviewed_diff_hash
                    )
                }
        });
        review_seen && qa_seen && spec_seen
    })
}
