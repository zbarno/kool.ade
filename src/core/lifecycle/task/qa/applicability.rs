use sha2::Digest as _;

use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskRecord};
use crate::runtime_qa::{QaApplicability, QaApplicabilityAssessmentV1};

pub fn valid_not_applicable(
    task: &TaskRecord,
    assessment: &QaApplicabilityAssessmentV1,
    reviewer_receipt: &GateReceipt,
    receipt: &GateReceipt,
) -> bool {
    let Ok(encoded) = serde_json::to_vec(assessment) else {
        return false;
    };
    let assessment_hash = format!("{:x}", sha2::Sha256::digest(encoded));
    assessment.validate().is_ok()
        && assessment.applicability == QaApplicability::NotApplicable
        && assessment.app_surface == crate::project_runtime::AppSurface::HeadlessService
        && assessment
            .evidence_refs
            .iter()
            .any(|item| item == &format!("target:{}", assessment.target_id))
        && reviewer_receipt.validate().is_ok()
        && reviewer_receipt.actor == GateActor::IndependentReviewer
        && reviewer_receipt.decision == GateDecision::Approved
        && reviewer_receipt.subject_uid == task.task_uid
        && reviewer_receipt.subject_sha256 == assessment_hash
        && reviewer_receipt.reviewer_run_id.as_deref()
            == Some(assessment.reviewer_execution_id.as_str())
        && reviewer_receipt.reviewer_session_id.as_deref()
            == Some(assessment.reviewer_session_id.as_str())
        && reviewer_receipt.author_run_id.as_deref() == Some(assessment.approved_by.as_str())
        && reviewer_receipt.author_session_id.as_deref()
            == Some(assessment.manager_session_id.as_str())
        && matches!(
            reviewer_receipt.prompt_role(),
            Some(
                crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer
                    | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2
                    | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3
            )
        )
        && reviewer_receipt
            .evidence_refs
            .iter()
            .any(|item| item == &format!("runtime-qa-applicability-assessment:{assessment_hash}"))
        && assessment.task_uid == task.task_uid
        && Some(assessment.code_sha.as_str()) == task.reviewed_code_head_sha.as_deref()
        && assessment.approved_spec_hash == task.approved_plan_sha256
        && receipt.validate().is_ok()
        && receipt.actor == GateActor::ApplicationPolicy
        && receipt.decision == GateDecision::NotApplicable
        && receipt.subject_uid == task.task_uid
        && Some(receipt.subject_sha256.as_str()) == task.implementation_sha256.as_deref()
        && receipt.reviewer_run_id.as_deref() == Some(assessment.approved_by.as_str())
        && receipt.reviewer_session_id.as_deref() == Some(assessment.manager_session_id.as_str())
        && receipt
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("gate-receipt:{}", reviewer_receipt.receipt_uid))
        && receipt
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("runtime-qa-applicability:{assessment_hash}"))
}
