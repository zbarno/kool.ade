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
    pub lifecycle_pr_work_uid: Option<String>,
    pub lifecycle_pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    pub lifecycle_pr_error: Option<String>,
    pub lifecycle_spec_proposal: Option<LifecycleSpecProposal>,
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
}
