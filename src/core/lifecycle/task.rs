use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use super::types::{GateActor, GateDecision, GateReceipt, TaskPhase, is_sha256, is_uuid};

mod events;
mod qa;
mod transitions;
pub use events::TaskEvent;
pub use qa::{QaExecutionStateV1, QaExecutionStatusV1};

const MAX_REVIEW_ATTEMPTS: usize = 8;
const MAX_REPAIR_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub schema_version: u16,
    pub task_uid: String,
    pub feature_uid: String,
    /// UID of the persisted planning item this lifecycle identity tracks.
    #[serde(default)]
    pub planning_item_uid: Option<String>,
    /// Hash of the task content, domain-bound to `approved_plan_sha256`.
    #[serde(default)]
    pub task_spec_sha256: Option<String>,
    /// Subtasks belong to exactly one standalone task and do not create their
    /// own pull request. `None` identifies a standalone task.
    #[serde(default)]
    pub parent_task_uid: Option<String>,
    /// Stable direct children owned by this standalone task.
    #[serde(default)]
    pub subtask_uids: Vec<String>,
    /// Approved implementation dependencies within the same task batch.
    #[serde(default)]
    pub dependency_task_uids: Vec<String>,
    pub phase: TaskPhase,
    pub revision: u64,
    pub approved_plan_sha256: String,
    /// Diff hash reviewed by CodeReviewer. Kept separate from the Git commit OID.
    pub implementation_sha256: Option<String>,
    /// Exact verified task-clone commit included in the CodeReviewer subject.
    #[serde(default)]
    pub reviewed_code_head_sha: Option<String>,
    /// Hash of the complete immutable CodeReviewer subject, including the head.
    #[serde(default)]
    pub reviewed_code_subject_sha256: Option<String>,
    pub code_review: Option<GateReceipt>,
    /// Append-only outcomes for independent reviews across repair cycles.
    #[serde(default)]
    pub review_history: Vec<GateReceipt>,
    #[serde(default)]
    pub repair_attempts: u8,
    pub runtime_qa: Option<GateReceipt>,
    #[serde(default)]
    pub qa_execution: Option<QaExecutionStateV1>,
    pub spec_reconciliation: Option<GateReceipt>,
    #[serde(default)]
    pub spec_reconciliation_blocker: Option<String>,
    #[serde(default)]
    pub spec_reconciliation_proposal_sha256: Option<String>,
    #[serde(default)]
    pub spec_reconciliation_spec_sha256: Option<String>,
    /// Canonical validated SpecDelta proposal awaiting explicit user resolution.
    #[serde(default)]
    pub spec_reconciliation_proposal_json: Option<String>,
    pub pr_work_item_uid: Option<String>,
}

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
                .is_some_and(|sha| !super::types::is_git_oid(sha))
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
