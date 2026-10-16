use sha2::Digest as _;

use super::super::types::{GateActor, GateDecision, is_sha256, is_uuid};
use super::{MAX_REPAIR_ATTEMPTS, MAX_REVIEW_ATTEMPTS, QaExecutionStatusV1, TaskPhase, TaskRecord};

impl TaskRecord {
    pub fn validate(&self) -> Result<(), String> {
        let invalid_proposal_json =
            self.spec_reconciliation_proposal_json
                .as_ref()
                .is_some_and(|json| {
                    let hash = format!("{:x}", sha2::Sha256::digest(json.as_bytes()));
                    json.len() > 64 * 1024
                        || serde_json::from_str::<crate::harness::prompt_catalog::SpecDeltaV1>(json)
                            .is_err()
                        || self.spec_reconciliation_proposal_sha256.as_deref()
                            != Some(hash.as_str())
                        || self.spec_reconciliation_blocker.is_none()
                });
        if self.schema_version != 1
            || !is_uuid(&self.task_uid)
            || !is_uuid(&self.feature_uid)
            || self
                .planning_item_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .task_spec_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self
                .parent_task_uid
                .as_deref()
                .is_some_and(|parent| !is_uuid(parent) || parent == self.task_uid)
            || !is_sha256(&self.approved_plan_sha256)
            || self
                .implementation_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self
                .reviewed_code_head_sha
                .as_deref()
                .is_some_and(|sha| !super::super::types::is_git_oid(sha))
            || self
                .reviewed_code_subject_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self.reviewed_code_head_sha.is_some() != self.reviewed_code_subject_sha256.is_some()
            || self
                .pr_work_item_uid
                .as_deref()
                .is_some_and(|id| !is_uuid(id))
            || (self.parent_task_uid.is_some() && self.pr_work_item_uid.is_some())
            || self.subtask_uids.len() > 256
            || self.dependency_task_uids.len() > 256
            || self
                .subtask_uids
                .iter()
                .any(|uid| !is_uuid(uid) || uid == &self.task_uid)
            || self
                .subtask_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| self.subtask_uids[..index].iter().any(|prior| prior == uid))
            || self
                .dependency_task_uids
                .iter()
                .any(|uid| !is_uuid(uid) || uid == &self.task_uid)
            || self
                .dependency_task_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| {
                    self.dependency_task_uids[..index]
                        .iter()
                        .any(|prior| prior == uid)
                })
            || self.approved_requirement_ids.len() > 256
            || self
                .approved_requirement_ids
                .iter()
                .any(|id| id.trim().is_empty() || id.len() > 128 || id.contains('\0'))
            || self
                .approved_requirement_ids
                .iter()
                .enumerate()
                .any(|(index, id)| {
                    self.approved_requirement_ids[..index]
                        .iter()
                        .any(|prior| prior == id)
                })
            || (self.parent_task_uid.is_some() && !self.subtask_uids.is_empty())
            || self
                .code_review
                .iter()
                .chain(self.runtime_qa.iter())
                .chain(self.spec_reconciliation.iter())
                .any(|receipt| receipt.validate().is_err())
            || self.review_history.len() > MAX_REVIEW_ATTEMPTS
            || self.review_history.iter().any(|receipt| {
                receipt.validate().is_err()
                    || receipt.actor != GateActor::IndependentReviewer
                    || receipt.subject_uid != self.task_uid
            })
            || self
                .review_history
                .iter()
                .enumerate()
                .any(|(index, receipt)| {
                    self.review_history[..index]
                        .iter()
                        .any(|prior| prior.receipt_uid == receipt.receipt_uid)
                })
            || self
                .qa_execution
                .as_ref()
                .is_some_and(|state| state.validate(self).is_err())
            || (self.runtime_qa.is_some()
                && !self
                    .qa_execution
                    .as_ref()
                    .is_some_and(|state| state.status == QaExecutionStatusV1::Completed))
            || self.repair_attempts > MAX_REPAIR_ATTEMPTS
            || self
                .spec_reconciliation_blocker
                .as_deref()
                .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 4096)
            || self
                .spec_reconciliation_proposal_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self
                .spec_reconciliation_spec_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self.spec_reconciliation_blocker.is_some()
                != self.spec_reconciliation_proposal_sha256.is_some()
            || self.spec_reconciliation_blocker.is_some()
                != self.spec_reconciliation_spec_sha256.is_some()
            || invalid_proposal_json
        {
            return Err("task lifecycle record is invalid".into());
        }
        if matches!(
            self.phase,
            TaskPhase::AwaitingReview
                | TaskPhase::Repairing
                | TaskPhase::ReReview
                | TaskPhase::AwaitingRuntimeQa
                | TaskPhase::AwaitingSpecReconciliation
                | TaskPhase::Done
                | TaskPhase::PrPending
                | TaskPhase::PrOpened
                | TaskPhase::PrMerged
        ) && self.implementation_sha256.is_none()
        {
            return Err("task phase requires an implementation revision".into());
        }
        if matches!(
            self.phase,
            TaskPhase::AwaitingRuntimeQa
                | TaskPhase::AwaitingSpecReconciliation
                | TaskPhase::Done
                | TaskPhase::PrPending
                | TaskPhase::PrOpened
                | TaskPhase::PrMerged
        ) && (self.reviewed_code_head_sha.is_none()
            || self.reviewed_code_subject_sha256.is_none())
        {
            return Err("task phase requires the reviewed implementation head".into());
        }
        if matches!(
            self.phase,
            TaskPhase::AwaitingRuntimeQa | TaskPhase::AwaitingSpecReconciliation
        ) && !self.code_review.as_ref().is_some_and(|receipt| {
            receipt.decision == GateDecision::Approved
                && Some(receipt.subject_sha256.as_str()) == self.implementation_sha256.as_deref()
        }) {
            return Err("task phase requires a passing review of the current code".into());
        }
        if self.phase == TaskPhase::AwaitingRuntimeQa
            && !self.qa_execution.as_ref().is_some_and(|state| {
                matches!(
                    state.status,
                    QaExecutionStatusV1::Pending | QaExecutionStatusV1::Blocked
                )
            })
        {
            return Err(
                "task phase requires a persisted pending or blocked runtime QA state".into(),
            );
        }
        if self.phase == TaskPhase::AwaitingSpecReconciliation
            && (!self
                .qa_execution
                .as_ref()
                .is_some_and(|state| state.status == QaExecutionStatusV1::Completed)
                || !self.runtime_qa.as_ref().is_some_and(|receipt| {
                    matches!(
                        receipt.decision,
                        GateDecision::Approved | GateDecision::NotApplicable
                    ) && Some(receipt.subject_sha256.as_str())
                        == self.implementation_sha256.as_deref()
                }))
        {
            return Err("task phase requires passing or not-applicable runtime QA".into());
        }
        if self.phase == TaskPhase::Done && !self.has_completion_gates() {
            return Err(
                "done task is missing current review, QA, spec, or PR preparation evidence".into(),
            );
        }
        Ok(())
    }

    fn has_completion_gates(&self) -> bool {
        let code_sha = self.implementation_sha256.as_deref().unwrap_or_default();
        self.code_review.as_ref().is_some_and(|receipt| {
            receipt.subject_uid == self.task_uid
                && receipt.actor == GateActor::IndependentReviewer
                && receipt.decision == GateDecision::Approved
                && receipt.subject_sha256 == code_sha
        }) && self
            .qa_execution
            .as_ref()
            .is_some_and(|state| state.status == QaExecutionStatusV1::Completed)
            && self.runtime_qa.as_ref().is_some_and(|receipt| {
                receipt.subject_uid == self.task_uid
                    && matches!(
                        receipt.actor,
                        GateActor::IndependentReviewer | GateActor::ApplicationPolicy
                    )
                    && matches!(
                        receipt.decision,
                        GateDecision::Approved | GateDecision::NotApplicable
                    )
                    && (receipt.decision != GateDecision::NotApplicable
                        || receipt.actor == GateActor::ApplicationPolicy)
                    && receipt.subject_sha256 == code_sha
            })
            && self.spec_reconciliation.as_ref().is_some_and(|receipt| {
                receipt.subject_uid == self.task_uid
                    && receipt.actor == GateActor::ApplicationPolicy
                    && receipt.decision == GateDecision::Approved
                    && receipt.subject_sha256 == code_sha
            })
            && (self.parent_task_uid.is_some() || self.pr_work_item_uid.is_some())
    }
}
