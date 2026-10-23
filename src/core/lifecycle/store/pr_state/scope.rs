use super::super::task_transition::{
    PrPublicationScopeV1, PrPublicationTaskEvidenceV1, PullRequestPreparation,
};
use super::super::{LifecycleStore, TaskRecord};
use crate::artifacts::planning_store::StoreError;

pub(super) fn matches_current(
    lifecycle: &LifecycleStore<'_>,
    work: &PullRequestPreparation,
    parent: &TaskRecord,
    scope: &PrPublicationScopeV1,
) -> Result<bool, StoreError> {
    let Some(spec_revision) = lifecycle.load_spec_revision(&parent.feature_uid)? else {
        return Ok(false);
    };
    if scope.source_sha != parent.reviewed_code_head_sha.as_deref().unwrap_or_default()
        || scope.reviewed_diff_hash != parent.implementation_sha256.as_deref().unwrap_or_default()
        || scope.accepted_spec_revision != spec_revision.seq
    {
        return Ok(false);
    }
    for (task_uid, evidence) in work.included_task_uids.iter().zip(&scope.task_evidence) {
        let task = if task_uid == &parent.task_uid {
            parent.clone()
        } else {
            lifecycle.load_task(task_uid)?
        };
        if !task_evidence_matches(&task, evidence) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn task_evidence_matches(task: &TaskRecord, evidence: &PrPublicationTaskEvidenceV1) -> bool {
    task.phase == crate::core::lifecycle::TaskPhase::Done
        && evidence.task_uid == task.task_uid
        && evidence.task_revision == task.revision
        && task.reviewed_code_head_sha.as_deref() == Some(evidence.reviewed_head_sha.as_str())
        && task.implementation_sha256.as_deref() == Some(evidence.reviewed_diff_hash.as_str())
        && task.code_review.as_ref() == Some(&evidence.code_review_receipt)
        && task.runtime_qa.as_ref() == Some(&evidence.qa_receipt)
        && task.spec_reconciliation.as_ref() == Some(&evidence.spec_sync_receipt)
}
