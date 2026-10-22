#[derive(Clone)]
pub struct LifecycleView {
    pub feature_uid: String,
    pub feature_phase: crate::core::lifecycle::FeaturePhase,
    pub task_phase: crate::core::lifecycle::TaskPhase,
    pub parent_task_uid: Option<String>,
    pub parent_task: Option<TaskRelationView>,
    pub dependencies: Vec<TaskRelationView>,
    pub subtasks: Vec<TaskRelationView>,
    pub task_revision: u64,
    pub code_review: Option<crate::core::lifecycle::GateReceipt>,
    pub code_review_attempts: Vec<crate::core::lifecycle::GateReceipt>,
    pub current_code_review: Option<CodeReviewInspectionView>,
    pub code_review_dispositions: Vec<CodeReviewDispositionView>,
    pub code_review_load_error: Option<String>,
    pub runtime_qa: Option<crate::core::lifecycle::GateReceipt>,
    pub runtime_qa_run_id: Option<String>,
    pub spec_reconciliation: Option<crate::core::lifecycle::GateReceipt>,
    pub current_spec_hash: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub approved_target_hash: Option<String>,
    pub deployed_spec_hash: Option<String>,
    pub deployment_status: Option<String>,
    pub deployment_observed_at_ms: Option<u64>,
    pub spec_revision: Option<u64>,
    pub spec_status: Option<crate::core::lifecycle::SpecReconciliationStatusV1>,
    pub pending_spec_triggers: usize,
    pub specification_deltas: Vec<(u64, crate::core::lifecycle::SpecDeltaRecordV1)>,
    pub pull_request_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    pub merged_commit_sha: Option<String>,
    pub runtime_qa_evidence: Option<RuntimeQaEvidenceView>,
    pub runtime_qa_evidence_error: Option<String>,
    pub runtime_qa_applicability: Option<crate::runtime_qa::QaApplicabilityAssessmentV1>,
}

#[derive(Clone)]
pub struct RuntimeQaEvidenceView {
    pub run_id: String,
    pub outcome: crate::runtime_qa::QaOutcome,
    pub coverage: Vec<crate::runtime_qa::QaRequirementCoverageV1>,
    pub findings: Vec<crate::runtime_qa::QaFindingV1>,
    pub actions: Vec<crate::runtime_qa::QaActionEvidenceV1>,
    pub screenshots: Vec<RuntimeQaScreenshotView>,
}

#[derive(Clone)]
pub struct RuntimeQaScreenshotView {
    pub id: String,
    pub scenario_id: String,
    pub sha256: String,
    pub width: u16,
    pub height: u16,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct RuntimeQaScreenshotGalleryView {
    pub total: usize,
    pub screenshots: Vec<RuntimeQaScreenshotView>,
}

#[derive(Clone)]
pub struct LifecyclePlanReviewView {
    pub feature_uid: String,
    pub snapshot: crate::core::lifecycle::PlanSnapshotV1,
    pub confirmation: Option<crate::core::lifecycle::GateReceipt>,
    pub review: Option<crate::core::lifecycle::GateReceipt>,
    pub verdict: Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
}

#[derive(Clone)]
pub struct CodeReviewInspectionView {
    pub task_uid: String,
    pub subject_hash: String,
    pub diff_hash: String,
    pub head_sha: String,
    pub prompt_hash: String,
    pub reviewer_run_id: String,
    pub reviewer_session_id: String,
    pub verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
}

#[derive(Clone)]
pub struct CodeReviewDispositionView {
    pub finding_id: String,
    pub subject_hash: String,
    pub scope: Option<crate::harness::prompt_catalog::FindingScopeV1>,
    pub status: crate::core::planning_work::CodeReviewDispositionStatusV1,
    pub work_id: String,
    pub title: String,
    pub work_status: crate::core::planning_work::WorkStatus,
    pub occurrence_count: u32,
}

#[derive(Clone)]
pub struct TaskRelationView {
    pub planning_item_uid: String,
    pub lifecycle_task_uid: Option<String>,
    pub title: String,
    pub status: String,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Clone)]
pub struct LifecycleSpecProposal {
    pub expected_revision: u64,
    pub proposal_sha256: String,
    pub base_spec_sha256: String,
    pub reason: String,
    pub proposal: Option<crate::harness::prompt_catalog::SpecDeltaV1>,
}

pub fn task_phase_label(phase: crate::core::lifecycle::TaskPhase) -> &'static str {
    use crate::core::lifecycle::TaskPhase as Phase;
    match phase {
        Phase::Ready => "Ready",
        Phase::Blocked => "Blocked",
        Phase::Running => "Running",
        Phase::AwaitingReview => "Awaiting review",
        Phase::Repairing => "Repairing",
        Phase::ReReview => "Review after repair",
        Phase::AwaitingRuntimeQa => "Awaiting runtime QA",
        Phase::AwaitingSpecReconciliation => "Awaiting spec sync",
        Phase::Done => "Implementation done",
        Phase::PrPending => "PR preparation",
        Phase::PrOpened => "PR opened",
        Phase::PrMerged => "PR merged",
        Phase::Failed => "Failed",
        Phase::Cancelled => "Cancelled",
    }
}

pub fn lifecycle_pr_publication_enabled(
    lifecycle_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    implementation_status: crate::core::implementation::ImplementationStatus,
    has_pull_request_url: bool,
) -> bool {
    use crate::core::{
        implementation::ImplementationStatus as Implementation,
        lifecycle::PullRequestPreparationStatus as Pr,
    };
    match lifecycle_status {
        Some(Pr::AwaitingAuthorization) => {
            matches!(
                implementation_status,
                Implementation::ReadyToPublish | Implementation::AwaitingApproval
            ) && !has_pull_request_url
        }
        Some(Pr::NeedsAttention) => {
            implementation_status == Implementation::Completed && !has_pull_request_url
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::lifecycle_pr_publication_enabled as enabled;
    use crate::core::{
        implementation::ImplementationStatus as Implementation,
        lifecycle::PullRequestPreparationStatus as Pr,
    };

    #[test]
    fn publication_retry_is_enabled_for_completed_needs_attention_work() {
        assert!(enabled(
            Some(Pr::NeedsAttention),
            Implementation::Completed,
            false
        ));
        assert!(!enabled(
            Some(Pr::NeedsAttention),
            Implementation::Completed,
            true
        ));
        assert!(!enabled(Some(Pr::Opened), Implementation::Completed, false));
    }

    #[test]
    fn initial_publication_requires_an_unpublished_reviewed_implementation() {
        assert!(enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::ReadyToPublish,
            false
        ));
        assert!(enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::AwaitingApproval,
            false
        ));
        assert!(!enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::Completed,
            false
        ));
        assert!(!enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::ReadyToPublish,
            true
        ));
    }
}
