use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskRecord};

impl TaskRecord {
    pub(super) fn valid_qa_request(&self, request: &crate::runtime_qa::QaRunRequestV1) -> bool {
        request.validate().is_ok()
            && request.parent_task_uid == self.task_uid
            && request.feature_uid == self.feature_uid
            && Some(request.code_head_sha.as_str()) == self.reviewed_code_head_sha.as_deref()
            && Some(request.complete_diff_hash.as_str()) == self.implementation_sha256.as_deref()
            && Some(request.review_subject_sha256.as_str())
                == self.reviewed_code_subject_sha256.as_deref()
            && request.approved_spec_hash == self.approved_plan_sha256
            && self.code_review.as_ref().is_some_and(|review| {
                review.decision == GateDecision::Approved
                    && review.subject_sha256 == request.complete_diff_hash
                    && review.author_run_id.as_deref() == Some(request.author_run_id.as_str())
                    && review.author_session_id.as_deref()
                        == Some(request.author_session_id.as_str())
            })
    }

    pub(super) fn valid_qa(&self, receipt: &GateReceipt) -> bool {
        self.implementation_sha256.as_deref() == Some(receipt.subject_sha256.as_str())
            && receipt.subject_uid == self.task_uid
            && receipt.actor == GateActor::IndependentReviewer
            && matches!(
                receipt.prompt_role(),
                Some(
                    crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer
                        | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2
                        | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3
                )
            )
            && receipt.validate().is_ok()
    }
}
