//! Read model and typed user commands for one task's detail surface.

use crate::domain::ChatMessage;

#[derive(Clone)]
pub struct ViewModel {
    pub implementation: Option<crate::core::implementation::Implementation>,
    pub implementation_active: bool,
    pub failure: Option<String>,
    pub failure_disposition: Option<crate::core::implementation::RecoveryDisposition>,
    pub attention: Option<crate::core::attention::View>,
    pub messages: Vec<ChatMessage>,
    pub progress: Option<crate::harness::LiveProgress>,
    pub conversation_active: bool,
    pub conversation_error: Option<String>,
    pub task_status: Option<crate::core::planning_work::WorkStatus>,
    pub board_column: usize,
    pub can_start: bool,
    pub auto_build: bool,
    pub draft: String,
    pub activity_samples: Vec<(i64, u64)>,
    pub activity_active: bool,
    pub implementation_metrics: Option<crate::persistence::telemetry::ImplementationMetrics>,
    pub feature_metrics: Option<crate::persistence::telemetry::ImplementationMetrics>,
    pub target_preference: crate::execution::contract::TargetPreference,
    pub task_capabilities:
        crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
    pub task_allowed_targets: Vec<crate::execution::contract::TargetPreference>,
    pub target_choices: Vec<TargetChoice>,
    pub remote_private_file_candidates: Vec<RemotePrivateFileGrantCandidate>,
    pub remote_private_file_candidates_error: Option<String>,
    pub remote_private_file_candidates_loading: bool,
    pub remote_private_file_workers: Vec<RemotePrivateFileGrantWorker>,
    pub remote_private_file_grants: Vec<RemotePrivateFileGrantView>,
    pub remote_private_file_grants_error: Option<String>,
    pub execution_evidence: Option<crate::persistence::execution_records::ExecutionRecordV1>,
    pub execution_evidence_error: Option<String>,
    pub lifecycle_pr_work_uid: Option<String>,
    pub lifecycle_pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    pub lifecycle_pr_error: Option<String>,
    pub lifecycle_spec_proposal: Option<LifecycleSpecProposal>,
    pub runtime_capability: Option<RuntimeCapabilityView>,
    /// Read-only lifecycle state projected from the persisted #157 records.
    pub lifecycle: Option<LifecycleView>,
    pub lifecycle_error: Option<String>,
}

#[derive(Clone)]
pub struct RemotePrivateFileGrantView {
    pub grant_id: String,
    pub worker_id: String,
    pub files: Vec<(
        String,
        crate::execution::remote::private_files::PrivateDataClassV1,
    )>,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone)]
pub struct RemotePrivateFileGrantCandidate {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub data_class: crate::execution::remote::private_files::PrivateDataClassV1,
}

#[derive(Clone)]
pub struct RemotePrivateFileGrantWorker {
    pub worker_id: String,
    pub destination: String,
}

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
}

#[derive(Clone)]
pub struct RuntimeQaScreenshotView {
    pub id: String,
    pub scenario_id: String,
    pub sha256: String,
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

#[derive(Clone)]
pub struct RuntimeCapabilityView {
    pub source_sha: Option<String>,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub manifest_revision: Option<u64>,
    pub selected_target_id: Option<String>,
    pub targets: Vec<RuntimeTargetChoice>,
    pub message: Option<String>,
}

#[derive(Clone)]
pub struct RuntimeTargetChoice {
    pub target_id: String,
    pub label: String,
    pub selected: bool,
    pub state: String,
    pub recipes: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRecipeOperation {
    Build,
    Test,
    Start,
}

#[derive(Clone)]
pub struct LifecycleSpecProposal {
    pub expected_revision: u64,
    pub proposal_sha256: String,
    pub base_spec_sha256: String,
    pub reason: String,
    pub proposal: Option<crate::harness::prompt_catalog::SpecDeltaV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetChoice {
    pub preference: crate::execution::contract::TargetPreference,
    pub label: String,
    pub available: bool,
    pub reason: String,
}

pub enum Command {
    UpdateDraft {
        ticket: String,
        draft: String,
    },
    SubmitReply {
        ticket: String,
        draft: String,
        decision: bool,
    },
    RetryChatSave,
    RetryExplanation {
        ticket: String,
        detail: String,
    },
    CancelTask {
        ticket: String,
    },
    StartOrResume {
        ticket: String,
    },
    PublishLifecyclePr {
        ticket: String,
    },
    ApproveSpecReconciliation {
        ticket: String,
        expected_revision: u64,
        proposal_sha256: String,
        base_spec_sha256: String,
    },
    RunWithoutSharedCoordination {
        ticket: String,
    },
    AuthorizeDependency {
        ticket: String,
        request_id: String,
        scope: crate::harness::DependencyAuthorizationScope,
    },
    DenyDependency {
        ticket: String,
        request_id: String,
    },
    SetExecutionTarget {
        ticket: String,
        preference: crate::execution::contract::TargetPreference,
    },
    SetTaskCapabilities {
        ticket: String,
        requirements:
            crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
    },
    SetTaskAllowedTargets {
        ticket: String,
        targets: Vec<crate::execution::contract::TargetPreference>,
    },
    GrantRemotePrivateFiles {
        ticket: String,
        worker_id: String,
        paths: Vec<String>,
    },
    RevokeRemotePrivateFileGrant {
        ticket: String,
        grant_id: String,
    },
    DiscoverRuntimeTargets {
        ticket: String,
    },
    SelectRuntimeTarget {
        ticket: String,
        target_id: String,
        expected_manifest_revision: u64,
    },
    ApproveRuntimeRecipe {
        ticket: String,
        target_id: String,
        operation: RuntimeRecipeOperation,
        executable_id: String,
        argv_lines: String,
        readiness_port: String,
        readiness_path: String,
        expected_manifest_revision: u64,
    },
}
