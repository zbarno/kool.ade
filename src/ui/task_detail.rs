//! Read model and typed user commands for one task's detail surface.

use crate::domain::ChatMessage;

mod runtime_capability;
pub use runtime_capability::*;
mod types;
pub use types::*;

#[derive(Clone)]
pub struct ViewModel {
    pub implementation: Option<crate::core::implementation::Implementation>,
    pub implementation_active: bool,
    pub failure: Option<String>,
    pub answer_context: Option<String>,
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
    pub remote_execution_telemetry:
        Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    pub remote_execution_telemetry_error: Option<String>,
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
    AskQuestion {
        ticket: String,
        draft: String,
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
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        executable_id: String,
        argv_lines: String,
        artifact_paths: String,
        readiness_port: String,
        readiness_path: String,
        expected_manifest_revision: u64,
    },
    ApplyRuntimeAdapter {
        ticket: String,
        target_id: String,
        operation: RuntimeRecipeOperation,
        expected_manifest_revision: u64,
    },
    AddRuntimeTarget {
        ticket: String,
        app_surface: crate::project_runtime::AppSurface,
        project_relative_root: String,
        fixture_ref: String,
        artifact_paths: String,
        build: RuntimeRecipeInput,
        test: RuntimeRecipeInput,
        start: Box<RuntimeRecipeInput>,
        required_dependencies: Vec<String>,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        readiness_port: String,
        readiness_path: String,
        expected_manifest_revision: u64,
    },
}
