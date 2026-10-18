use serde::{Deserialize, Serialize};

use crate::runtime_qa::QaRunRequestV1;

use super::TaskRecord;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, types::is_sha256};

mod applicability;
pub(super) use applicability::valid_not_applicable;

pub(super) struct QaCompletion {
    pub qa_run_id: String,
    pub result_sha256: String,
    pub evidence_manifest_sha256: String,
    pub reviewer_execution_id: String,
    pub reviewer_session_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaExecutionStatusV1 {
    Pending,
    Blocked,
    Completed,
    NotApplicable,
}

/// Durable pointer to one independently executed QA attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QaExecutionStateV1 {
    pub schema_version: u16,
    pub status: QaExecutionStatusV1,
    pub request: Option<QaRunRequestV1>,
    pub blocked_reason: Option<String>,
    pub result_sha256: Option<String>,
    pub evidence_manifest_sha256: Option<String>,
    pub reviewer_execution_id: Option<String>,
    pub reviewer_session_id: Option<String>,
    #[serde(default)]
    pub applicability: Option<crate::runtime_qa::QaApplicabilityAssessmentV1>,
}

impl QaExecutionStateV1 {
    pub(super) fn pending() -> Self {
        Self {
            schema_version: 1,
            status: QaExecutionStatusV1::Pending,
            request: None,
            blocked_reason: None,
            result_sha256: None,
            evidence_manifest_sha256: None,
            reviewer_execution_id: None,
            reviewer_session_id: None,
            applicability: None,
        }
    }

    pub(super) fn validate(&self, task: &TaskRecord) -> Result<(), String> {
        let request_valid = self.request.as_ref().is_some_and(|request| {
            request.validate().is_ok()
                && request.parent_task_uid == task.task_uid
                && request.feature_uid == task.feature_uid
                && Some(request.code_head_sha.as_str()) == task.reviewed_code_head_sha.as_deref()
                && Some(request.complete_diff_hash.as_str())
                    == task.implementation_sha256.as_deref()
                && Some(request.review_subject_sha256.as_str())
                    == task.reviewed_code_subject_sha256.as_deref()
                && request.approved_spec_hash == task.approved_plan_sha256
                && task.code_review.as_ref().is_some_and(|review| {
                    review.decision == GateDecision::Approved
                        && review.subject_sha256 == request.complete_diff_hash
                        && review.author_run_id.as_deref() == Some(request.author_run_id.as_str())
                        && review.author_session_id.as_deref()
                            == Some(request.author_session_id.as_str())
                })
        });
        let valid = match self.status {
            QaExecutionStatusV1::Pending => {
                self.blocked_reason.is_none()
                    && self.result_sha256.is_none()
                    && self.evidence_manifest_sha256.is_none()
                    && self.reviewer_execution_id.is_none()
                    && self.reviewer_session_id.is_none()
                    && self.applicability.is_none()
                    && (self.request.is_none() || request_valid)
            }
            QaExecutionStatusV1::Blocked => {
                self.blocked_reason
                    .as_ref()
                    .is_some_and(|reason| !reason.trim().is_empty() && reason.len() <= 2048)
                    && (request_valid
                        || (self.request.is_none()
                            && self.result_sha256.is_none()
                            && self.evidence_manifest_sha256.is_none()))
                    && self
                        .result_sha256
                        .as_ref()
                        .is_none_or(|hash| is_sha256(hash))
                    && self
                        .evidence_manifest_sha256
                        .as_ref()
                        .is_none_or(|hash| is_sha256(hash))
                    && self.result_sha256.is_some() == self.evidence_manifest_sha256.is_some()
                    && self.reviewer_execution_id.is_none()
                    && self.reviewer_session_id.is_none()
                    && self.applicability.is_none()
            }
            QaExecutionStatusV1::Completed => {
                request_valid
                    && self.blocked_reason.is_none()
                    && self.result_sha256.as_deref().is_some_and(is_sha256)
                    && self
                        .evidence_manifest_sha256
                        .as_deref()
                        .is_some_and(is_sha256)
                    && self.reviewer_execution_id.as_deref().is_some_and(is_uuid)
                    && self.reviewer_session_id.as_deref().is_some_and(is_uuid)
                    && self.applicability.is_none()
            }
            QaExecutionStatusV1::NotApplicable => {
                self.request.is_none()
                    && self.blocked_reason.is_none()
                    && self.result_sha256.is_none()
                    && self.evidence_manifest_sha256.is_none()
                    && self.reviewer_execution_id.is_none()
                    && self.reviewer_session_id.is_none()
                    && self.applicability.as_ref().is_some_and(|assessment| {
                        assessment.validate().is_ok()
                            && assessment.applicability
                                == crate::runtime_qa::QaApplicability::NotApplicable
                            && assessment.task_uid == task.task_uid
                            && Some(assessment.code_sha.as_str())
                                == task.reviewed_code_head_sha.as_deref()
                            && assessment.approved_spec_hash == task.approved_plan_sha256
                    })
            }
        };
        if self.schema_version != 1 || !valid {
            return Err("runtime QA lifecycle state is stale or malformed".into());
        }
        Ok(())
    }

    pub(super) fn record_block(
        &mut self,
        request_id: &str,
        reason: String,
        result_sha256: Option<String>,
        evidence_manifest_sha256: Option<String>,
    ) -> Result<(), String> {
        if self.status != QaExecutionStatusV1::Pending
            || self
                .request
                .as_ref()
                .map(|request| request.qa_run_id.as_str())
                != Some(request_id)
            || reason.trim().is_empty()
            || reason.len() > 2048
            || result_sha256.as_ref().is_some_and(|hash| !is_sha256(hash))
            || evidence_manifest_sha256
                .as_ref()
                .is_some_and(|hash| !is_sha256(hash))
            || result_sha256.is_some() != evidence_manifest_sha256.is_some()
        {
            return Err("runtime QA block does not match the pending request".into());
        }
        self.status = QaExecutionStatusV1::Blocked;
        self.applicability = None;
        self.blocked_reason = Some(reason);
        self.result_sha256 = result_sha256;
        self.evidence_manifest_sha256 = evidence_manifest_sha256;
        Ok(())
    }

    pub(super) fn record_unavailable(&mut self, reason: String) -> Result<(), String> {
        if self.status != QaExecutionStatusV1::Pending
            || self.request.is_some()
            || reason.trim().is_empty()
            || reason.len() > 2048
        {
            return Err("runtime QA unavailability is not a pending request blocker".into());
        }
        self.status = QaExecutionStatusV1::Blocked;
        self.applicability = None;
        self.blocked_reason = Some(reason);
        Ok(())
    }

    pub(super) fn record_completion(
        &mut self,
        task_uid: &str,
        completion: QaCompletion,
        receipt: &GateReceipt,
    ) -> Result<(), String> {
        let QaCompletion {
            qa_run_id,
            result_sha256,
            evidence_manifest_sha256,
            reviewer_execution_id,
            reviewer_session_id,
        } = completion;
        let request = self
            .request
            .as_ref()
            .ok_or("runtime QA request is missing")?;
        if self.status != QaExecutionStatusV1::Pending
            || request.qa_run_id != qa_run_id
            || !is_sha256(&result_sha256)
            || !is_sha256(&evidence_manifest_sha256)
            || !is_uuid(&reviewer_execution_id)
            || !is_uuid(&reviewer_session_id)
            || request.author_run_id == reviewer_execution_id
            || request.author_session_id == reviewer_session_id
            || receipt.actor != GateActor::IndependentReviewer
            || !matches!(
                receipt.decision,
                GateDecision::Approved | GateDecision::Rejected | GateDecision::ChangesRequested
            )
            || receipt.subject_uid != task_uid
            || receipt.subject_sha256 != request.complete_diff_hash
            || receipt.author_run_id.as_deref() != Some(request.author_run_id.as_str())
            || receipt.author_session_id.as_deref() != Some(request.author_session_id.as_str())
            || receipt.reviewer_run_id.as_deref() != Some(&reviewer_execution_id)
            || receipt.reviewer_session_id.as_deref() != Some(&reviewer_session_id)
            || !receipt
                .evidence_refs
                .iter()
                .any(|item| item == &format!("runtime-qa:{qa_run_id}/result"))
            || !receipt.evidence_refs.iter().any(|item| {
                item == &format!("runtime-qa:{qa_run_id}/evidence:{evidence_manifest_sha256}")
            })
        {
            return Err(
                "runtime QA completion is not bound to its exact independent report".into(),
            );
        }
        self.status = QaExecutionStatusV1::Completed;
        self.applicability = None;
        self.blocked_reason = None;
        self.result_sha256 = Some(result_sha256);
        self.evidence_manifest_sha256 = Some(evidence_manifest_sha256);
        self.reviewer_execution_id = Some(reviewer_execution_id);
        self.reviewer_session_id = Some(reviewer_session_id);
        Ok(())
    }

    pub(super) fn record_not_applicable(
        &mut self,
        assessment: crate::runtime_qa::QaApplicabilityAssessmentV1,
    ) -> Result<(), String> {
        let eligible = self.status == QaExecutionStatusV1::Pending
            || (self.status == QaExecutionStatusV1::Blocked && self.request.is_none());
        if !eligible
            || self.request.is_some()
            || assessment.validate().is_err()
            || assessment.applicability != crate::runtime_qa::QaApplicability::NotApplicable
        {
            return Err("runtime QA applicability decision is not pending or valid".into());
        }
        self.status = QaExecutionStatusV1::NotApplicable;
        self.blocked_reason = None;
        self.result_sha256 = None;
        self.evidence_manifest_sha256 = None;
        self.reviewer_execution_id = None;
        self.reviewer_session_id = None;
        self.applicability = Some(assessment);
        Ok(())
    }
}

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
