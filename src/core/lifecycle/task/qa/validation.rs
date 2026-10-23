use crate::core::lifecycle::{GateDecision, TaskRecord, types::is_sha256};

use super::{QaExecutionStateV1, QaExecutionStatusV1, is_uuid};

impl QaExecutionStateV1 {
    pub(in crate::core::lifecycle::task) fn validate(
        &self,
        task: &TaskRecord,
    ) -> Result<(), String> {
        let request_revision_valid = match (&self.request, self.request_task_revision) {
            (None, None) | (Some(_), None) => true,
            (Some(_), Some(revision)) => revision < task.revision,
            (None, Some(_)) => false,
        };
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
                self.dispatch_task_revision.is_none()
                    && request_revision_valid
                    && self.blocked_reason.is_none()
                    && self.result_sha256.is_none()
                    && self.evidence_manifest_sha256.is_none()
                    && self.reviewer_execution_id.is_none()
                    && self.reviewer_session_id.is_none()
                    && self.applicability.is_none()
                    && (self.request.is_none() || request_valid)
            }
            QaExecutionStatusV1::Blocked => {
                request_revision_valid
                    && self
                        .blocked_reason
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
            QaExecutionStatusV1::Running => {
                request_valid
                    && self.request_task_revision.is_none_or(|request_revision| {
                        self.dispatch_task_revision
                            .is_some_and(|dispatch_revision| request_revision < dispatch_revision)
                    })
                    && self
                        .dispatch_task_revision
                        .is_some_and(|revision| revision < task.revision)
                    && self.blocked_reason.is_none()
                    && self.result_sha256.is_none()
                    && self.evidence_manifest_sha256.is_none()
                    && self.reviewer_execution_id.is_none()
                    && self.reviewer_session_id.is_none()
                    && self.applicability.is_none()
            }
            QaExecutionStatusV1::Completed => {
                request_revision_valid
                    && request_valid
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
                    && self.request_task_revision.is_none()
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
}
