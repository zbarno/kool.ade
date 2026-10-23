use super::super::{TaskEvent, TaskPhase, TaskRecord};
use crate::core::lifecycle::types::{is_git_oid, is_sha256};

pub(super) fn apply(task: &mut TaskRecord, event: &TaskEvent) -> bool {
    match event {
        TaskEvent::PublicationHeadChanged {
            code_sha256,
            head_sha,
        } if task.phase == TaskPhase::Done
            && is_sha256(code_sha256)
            && is_git_oid(head_sha)
            && task.reviewed_code_head_sha.as_deref() != Some(head_sha.as_str())
            && task.pr_work_item_uid.is_some() =>
        {
            task.implementation_sha256 = Some(code_sha256.clone());
            task.reviewed_code_head_sha = None;
            task.reviewed_code_subject_sha256 = None;
            task.code_review = None;
            task.runtime_qa = None;
            task.qa_execution = None;
            task.spec_reconciliation = None;
            task.spec_reconciliation_blocker = None;
            task.spec_reconciliation_proposal_sha256 = None;
            task.spec_reconciliation_spec_sha256 = None;
            task.spec_reconciliation_proposal_json = None;
            task.phase = TaskPhase::ReReview;
            true
        }
        TaskEvent::PublicationGatesRefreshed { receipt }
            if task.phase == TaskPhase::AwaitingSpecReconciliation
                && task.parent_task_uid.is_none()
                && task.pr_work_item_uid.is_some()
                && task.valid_spec_receipt(receipt) =>
        {
            task.spec_reconciliation = Some(receipt.clone());
            task.spec_reconciliation_blocker = None;
            task.spec_reconciliation_proposal_sha256 = None;
            task.spec_reconciliation_spec_sha256 = None;
            task.spec_reconciliation_proposal_json = None;
            task.phase = TaskPhase::Done;
            true
        }
        _ => false,
    }
}
