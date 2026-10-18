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
    pub target_choices: Vec<TargetChoice>,
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
    pub runtime_qa_scenario_proposal: Option<RuntimeQaScenarioProposal>,
    pub runtime_qa_applicability_proposal: Option<RuntimeQaApplicabilityProposal>,
    pub runtime_qa_applicability_error: Option<String>,
    pub runtime_qa_applicability_reviewing: bool,
}

#[derive(Clone)]
pub struct RuntimeQaApplicabilityProposal {
    pub proposal: crate::runtime_qa::QaApplicabilityProposalV1,
    pub proposal_sha256: String,
    pub stale: bool,
}

#[derive(Clone)]
pub struct RuntimeQaScenarioProposal {
    pub task_revision: u64,
    pub proposal_sha256: Option<String>,
    pub scenarios: Vec<crate::runtime_qa::QaScenarioV1>,
    pub approved: bool,
    pub stale: bool,
}

#[derive(Clone)]
pub struct LifecycleView {
    pub feature_phase: crate::core::lifecycle::FeaturePhase,
    pub task_phase: crate::core::lifecycle::TaskPhase,
    pub parent_task_uid: Option<String>,
    pub parent_task: Option<TaskRelationView>,
    pub dependencies: Vec<TaskRelationView>,
    pub subtasks: Vec<TaskRelationView>,
    pub task_revision: u64,
    pub code_review: Option<crate::core::lifecycle::GateReceipt>,
    pub runtime_qa: Option<crate::core::lifecycle::GateReceipt>,
    pub spec_reconciliation: Option<crate::core::lifecycle::GateReceipt>,
    pub current_spec_hash: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub approved_target_hash: Option<String>,
    pub deployed_spec_hash: Option<String>,
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
    pub width: u16,
    pub height: u16,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct TaskRelationView {
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
    pub app_surface: crate::project_runtime::AppSurface,
    pub selected: bool,
    pub state: String,
    pub recipes: String,
    pub qa_applicability: crate::project_runtime_capability::QaApplicability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRecipeOperation {
    Build,
    Test,
    Start,
}

#[derive(Clone, Default)]
pub struct RuntimeRecipeInput {
    pub executable_id: String,
    pub argv_lines: String,
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
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        executable_id: String,
        argv_lines: String,
        readiness_port: String,
        readiness_path: String,
        expected_manifest_revision: u64,
    },
    AddRuntimeTarget {
        ticket: String,
        app_surface: crate::project_runtime::AppSurface,
        project_relative_root: String,
        artifact_paths: String,
        required_dependency_ids: String,
        build: RuntimeRecipeInput,
        test: RuntimeRecipeInput,
        start: RuntimeRecipeInput,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        readiness_port: String,
        readiness_path: String,
        expected_manifest_revision: u64,
    },
}
