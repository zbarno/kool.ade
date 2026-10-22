mod publication;
mod qa;

use super::{
    GateReceipt, MAX_REPAIR_ATTEMPTS, MAX_REVIEW_ATTEMPTS, QaExecutionStateV1, QaExecutionStatusV1,
    TaskEvent, TaskPhase, TaskRecord,
};
use crate::core::lifecycle::types::{GateActor, GateDecision, is_sha256, is_uuid};

impl TaskRecord {
    pub fn apply(&mut self, expected_revision: u64, event: TaskEvent) -> Result<(), String> {
        self.validate()?;
        if self.revision != expected_revision {
            return Err("task revision is stale".into());
        }
        if self.revision == u64::MAX {
            return Err("task revision is exhausted".into());
        }
        if publication::apply(self, &event) {
            self.revision += 1;
            return self.validate();
        }
        match event {
            TaskEvent::Start if self.phase == TaskPhase::Ready => {
                self.phase = TaskPhase::Running;
            }
            TaskEvent::ImplementationFinished { code_sha256 }
                if self.phase == TaskPhase::Running && is_sha256(&code_sha256) =>
            {
                self.implementation_sha256 = Some(code_sha256);
                self.reviewed_code_head_sha = None;
                self.reviewed_code_subject_sha256 = None;
                self.code_review = None;
                self.runtime_qa = None;
                self.qa_execution = None;
                self.spec_reconciliation = None;
                self.pr_work_item_uid = None;
                self.phase = if self.repair_attempts > 0 {
                    TaskPhase::ReReview
                } else {
                    TaskPhase::AwaitingReview
                };
            }
            TaskEvent::CodeReview {
                receipt,
                reviewed_head_sha,
                review_subject_sha256,
            } if matches!(
                self.phase,
                TaskPhase::AwaitingReview | TaskPhase::ReReview | TaskPhase::AwaitingRuntimeQa
            ) && self.valid_review(&receipt, &reviewed_head_sha, &review_subject_sha256) =>
            {
                if self.review_history.len() == MAX_REVIEW_ATTEMPTS
                    || self
                        .review_history
                        .iter()
                        .any(|prior| prior.receipt_uid == receipt.receipt_uid)
                {
                    return Err("review history is exhausted or receipt was replayed".into());
                }
                self.review_history.push(receipt.clone());
                self.reviewed_code_head_sha = Some(reviewed_head_sha);
                self.reviewed_code_subject_sha256 = Some(review_subject_sha256);
                match receipt.decision {
                    GateDecision::Approved => {
                        self.code_review = Some(receipt);
                        self.qa_execution = Some(QaExecutionStateV1::pending());
                        self.phase = TaskPhase::AwaitingRuntimeQa;
                    }
                    GateDecision::Rejected | GateDecision::ChangesRequested => {
                        self.code_review = Some(receipt);
                        self.phase = TaskPhase::Repairing;
                    }
                    GateDecision::NotApplicable => unreachable!(),
                }
                self.runtime_qa = None;
                self.spec_reconciliation = None;
            }
            TaskEvent::BeginRepair
                if self.phase == TaskPhase::Repairing
                    && self.repair_attempts < MAX_REPAIR_ATTEMPTS =>
            {
                self.repair_attempts += 1;
                self.phase = TaskPhase::Running;
                self.code_review = None;
                self.reviewed_code_head_sha = None;
                self.reviewed_code_subject_sha256 = None;
                self.runtime_qa = None;
                self.qa_execution = None;
                self.spec_reconciliation = None;
            }
            TaskEvent::RuntimeQaRequested(request)
                if self.phase == TaskPhase::AwaitingRuntimeQa
                    && self.valid_qa_request(&request) =>
            {
                let qa = self
                    .qa_execution
                    .get_or_insert_with(QaExecutionStateV1::pending);
                if qa.status == QaExecutionStatusV1::Completed {
                    return Err("completed runtime QA cannot be replaced".into());
                }
                qa.request = Some(request);
                qa.applicability = None;
                qa.status = QaExecutionStatusV1::Pending;
                qa.blocked_reason = None;
                qa.result_sha256 = None;
                qa.evidence_manifest_sha256 = None;
                qa.reviewer_execution_id = None;
                qa.reviewer_session_id = None;
                qa.applicability = None;
            }
            TaskEvent::RuntimeQaBlocked {
                request_id,
                reason,
                result_sha256,
                evidence_manifest_sha256,
            } if self.phase == TaskPhase::AwaitingRuntimeQa => {
                self.qa_execution
                    .as_mut()
                    .ok_or("runtime QA state is missing")?
                    .record_block(&request_id, reason, result_sha256, evidence_manifest_sha256)?;
            }
            TaskEvent::RuntimeQaUnavailable { reason }
                if self.phase == TaskPhase::AwaitingRuntimeQa =>
            {
                self.qa_execution
                    .as_mut()
                    .ok_or("runtime QA state is missing")?
                    .record_unavailable(reason)?;
            }
            TaskEvent::RuntimeQaCompleted {
                receipt,
                request_id,
                result_sha256,
                evidence_manifest_sha256,
                reviewer_execution_id,
                reviewer_session_id,
            } if self.phase == TaskPhase::AwaitingRuntimeQa && self.valid_qa(&receipt) => {
                self.qa_execution
                    .as_mut()
                    .ok_or("runtime QA state is missing")?
                    .record_completion(
                        &self.task_uid,
                        super::qa::QaCompletion {
                            qa_run_id: request_id,
                            result_sha256,
                            evidence_manifest_sha256,
                            reviewer_execution_id,
                            reviewer_session_id,
                        },
                        &receipt,
                    )?;
                match receipt.decision {
                    GateDecision::Approved => {
                        self.runtime_qa = Some(receipt);
                        self.phase = TaskPhase::AwaitingSpecReconciliation;
                    }
                    GateDecision::Rejected | GateDecision::ChangesRequested => {
                        self.runtime_qa = Some(receipt);
                        self.phase = TaskPhase::Repairing;
                    }
                    GateDecision::NotApplicable => {
                        return Err("independent runtime QA cannot be marked not applicable".into());
                    }
                }
                self.spec_reconciliation = None;
            }
            TaskEvent::RuntimeQaNotApplicable {
                assessment,
                reviewer_receipt,
                receipt,
            } if self.phase == TaskPhase::AwaitingRuntimeQa
                && super::qa::valid_not_applicable(
                    self,
                    &assessment,
                    &reviewer_receipt,
                    &receipt,
                ) =>
            {
                self.qa_execution
                    .as_mut()
                    .ok_or("runtime QA state is missing")?
                    .record_not_applicable(*assessment)?;
                self.runtime_qa = Some(*receipt);
                self.phase = TaskPhase::AwaitingSpecReconciliation;
                self.spec_reconciliation = None;
            }
            TaskEvent::ReconcileSpec {
                receipt,
                pr_work_item_uid,
            } if self.phase == TaskPhase::AwaitingSpecReconciliation
                && self.parent_task_uid.is_none()
                && self.valid_spec_receipt(&receipt)
                && is_uuid(&pr_work_item_uid) =>
            {
                self.spec_reconciliation = Some(receipt);
                self.spec_reconciliation_blocker = None;
                self.spec_reconciliation_proposal_sha256 = None;
                self.spec_reconciliation_spec_sha256 = None;
                self.spec_reconciliation_proposal_json = None;
                self.pr_work_item_uid = Some(pr_work_item_uid);
                self.phase = TaskPhase::Done;
            }
            TaskEvent::SpecReconciliationBlocked {
                reason,
                proposal_sha256,
                spec_sha256,
                proposal_json,
            } if self.phase == TaskPhase::AwaitingSpecReconciliation
                && self.valid_spec_blocker(&reason, &proposal_sha256, &spec_sha256) =>
            {
                self.spec_reconciliation_blocker = Some(reason);
                self.spec_reconciliation_proposal_sha256 = Some(proposal_sha256);
                self.spec_reconciliation_spec_sha256 = Some(spec_sha256);
                self.spec_reconciliation_proposal_json = proposal_json;
            }
            TaskEvent::SpecReconciliationRetry {
                current_spec_sha256,
            } if self.phase == TaskPhase::AwaitingSpecReconciliation
                && is_sha256(&current_spec_sha256)
                && self.spec_reconciliation_blocker.is_some()
                && self.spec_reconciliation_spec_sha256.as_deref()
                    != Some(current_spec_sha256.as_str()) =>
            {
                self.spec_reconciliation_blocker = None;
                self.spec_reconciliation_proposal_sha256 = None;
                self.spec_reconciliation_spec_sha256 = None;
                self.spec_reconciliation_proposal_json = None;
            }
            TaskEvent::SubtaskReconciled { receipt }
                if self.phase == TaskPhase::AwaitingSpecReconciliation
                    && self.parent_task_uid.is_some()
                    && self.valid_spec_receipt(&receipt) =>
            {
                self.spec_reconciliation = Some(receipt);
                self.pr_work_item_uid = None;
                self.phase = TaskPhase::Done;
            }
            TaskEvent::Cancel
                if !matches!(
                    self.phase,
                    TaskPhase::Done
                        | TaskPhase::PrPending
                        | TaskPhase::PrOpened
                        | TaskPhase::PrMerged
                        | TaskPhase::Failed
                        | TaskPhase::Cancelled
                ) =>
            {
                self.phase = TaskPhase::Cancelled;
            }
            _ => return Err("task lifecycle transition is not allowed".into()),
        }
        self.revision += 1;
        self.validate()
    }

    fn valid_review(
        &self,
        receipt: &GateReceipt,
        reviewed_head_sha: &str,
        review_subject_sha256: &str,
    ) -> bool {
        self.implementation_sha256.as_deref() == Some(receipt.subject_sha256.as_str())
            && super::super::types::is_git_oid(reviewed_head_sha)
            && is_sha256(review_subject_sha256)
            && receipt.subject_uid == self.task_uid
            && receipt.actor == GateActor::IndependentReviewer
            && receipt.prompt_role().is_some_and(|role| {
                matches!(
                    role,
                    crate::harness::prompt_catalog::PromptRole::CodeReviewer
                        | crate::harness::prompt_catalog::PromptRole::CodeReviewerV1
                )
            })
            && receipt.validate().is_ok()
    }

    fn valid_spec_receipt(&self, receipt: &GateReceipt) -> bool {
        self.implementation_sha256.as_deref() == Some(receipt.subject_sha256.as_str())
            && receipt.subject_uid == self.task_uid
            && receipt.actor == GateActor::ApplicationPolicy
            && receipt.decision == GateDecision::Approved
            && receipt.validate().is_ok()
    }

    fn valid_spec_blocker(&self, reason: &str, proposal_sha256: &str, spec_sha256: &str) -> bool {
        !reason.trim().is_empty()
            && reason.len() <= 4096
            && is_sha256(proposal_sha256)
            && is_sha256(spec_sha256)
            && self
                .qa_execution
                .as_ref()
                .is_some_and(|state| state.status == super::QaExecutionStatusV1::Completed)
            && self.runtime_qa.as_ref().is_some_and(|receipt| {
                receipt.decision == GateDecision::Approved
                    && Some(receipt.subject_sha256.as_str())
                        == self.implementation_sha256.as_deref()
            })
    }
}
