use serde::{Deserialize, Serialize};

use super::request::ExecutionModeV1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HarnessRole {
    Manager,
    #[serde(alias = "specification", alias = "investigation")]
    PlanningAnalyst,
    #[serde(alias = "decision_explanation")]
    PlanReviewer,
    DecisionExplainer,
    #[serde(alias = "task_generation")]
    TaskDecomposer,
    #[serde(alias = "implementation")]
    Implementer,
    CodeReviewer,
    #[serde(alias = "quality_assurance")]
    RuntimeQaReviewer,
    #[serde(alias = "reconciliation")]
    SpecReconciler,
    RepairWorker,
    #[serde(alias = "documentation")]
    PRPreparer,
}

impl HarnessRole {
    /// Every provider capability snapshot must state readiness for each role.
    /// `Unavailable` is explicit; omitting a role is never treated as support.
    pub const ALL: [Self; 11] = [
        Self::Manager,
        Self::PlanningAnalyst,
        Self::PlanReviewer,
        Self::DecisionExplainer,
        Self::TaskDecomposer,
        Self::Implementer,
        Self::CodeReviewer,
        Self::RuntimeQaReviewer,
        Self::SpecReconciler,
        Self::RepairWorker,
        Self::PRPreparer,
    ];

    pub fn harness_mode(self) -> crate::harness::ExecutionMode {
        use crate::harness::ExecutionMode;
        match self {
            Self::Manager => ExecutionMode::Planning,
            Self::PlanningAnalyst => ExecutionMode::Planning,
            Self::PlanReviewer | Self::CodeReviewer => ExecutionMode::Investigation,
            Self::DecisionExplainer => ExecutionMode::DecisionExplanation,
            Self::RuntimeQaReviewer => ExecutionMode::ReadOnlyAnalysis,
            Self::TaskDecomposer => ExecutionMode::TaskGeneration,
            Self::Implementer | Self::RepairWorker => ExecutionMode::Implementation,
            Self::PRPreparer => ExecutionMode::ReadOnlyAnalysis,
            Self::SpecReconciler => ExecutionMode::Reconciliation,
        }
    }

    /// Container tool authority for the role; provider selection cannot alter it.
    pub fn execution_mode(self) -> ExecutionModeV1 {
        match self {
            Self::Implementer | Self::RepairWorker => ExecutionModeV1::Implementation,
            Self::PRPreparer => ExecutionModeV1::PlanningReadOnly,
            Self::RuntimeQaReviewer => ExecutionModeV1::RuntimeQa,
            Self::Manager
            | Self::PlanningAnalyst
            | Self::PlanReviewer
            | Self::DecisionExplainer
            | Self::TaskDecomposer
            | Self::CodeReviewer
            | Self::SpecReconciler => ExecutionModeV1::PlanningReadOnly,
        }
    }

    /// Roles that can use repository or command tools require the application
    /// execution bridge. Context-only roles remain available without Docker.
    pub fn requires_execution_target(self) -> bool {
        self == Self::RuntimeQaReviewer
            || self.harness_mode().tool_access() != crate::harness::ToolAccess::None
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    Ready,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoleReadinessV1 {
    pub role: HarnessRole,
    pub available: ReadinessState,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModeReadinessV1 {
    pub mode: crate::harness::ExecutionMode,
    pub available: ReadinessState,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessCapabilitySnapshotV1 {
    pub schema_version: u16,
    pub provider_id: String,
    pub probe_id: String,
    pub probe_cancellation_generation: u64,
    pub probe_sequence: u64,
    pub cli_binary: ReadinessState,
    pub cli_version: Option<String>,
    pub authentication: ReadinessState,
    pub model_catalog: ReadinessState,
    pub scriptable_invocation: ReadinessState,
    pub docker_mcp_bridge: ReadinessState,
    pub execution_target: ReadinessState,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
    pub modes: Vec<ModeReadinessV1>,
    pub roles: Vec<RoleReadinessV1>,
}

/// Compatibility name retained while callers migrate to the capability contract.
pub type ProviderReadinessV1 = HarnessCapabilitySnapshotV1;

impl HarnessCapabilitySnapshotV1 {
    /// Whether the provider and application can satisfy an operation mode.
    /// Context-only modes do not require a Docker target.
    pub fn supports_mode(&self, mode: crate::harness::ExecutionMode) -> bool {
        if self.validate().is_err() {
            return false;
        }
        let provider_ready = self.cli_binary == ReadinessState::Ready
            && self
                .cli_version
                .as_ref()
                .is_some_and(|version| !version.trim().is_empty())
            && self.authentication == ReadinessState::Ready
            && self.model_catalog == ReadinessState::Ready
            && self.scriptable_invocation == ReadinessState::Ready;
        let mode_ready = self
            .modes
            .iter()
            .find(|readiness| readiness.mode == mode)
            .is_some_and(|readiness| readiness.available == ReadinessState::Ready);
        let execution_ready = matches!(
            mode.tool_access(),
            crate::harness::ToolAccess::None | crate::harness::ToolAccess::RuntimeQaScreenshots
        ) || (self.docker_mcp_bridge == ReadinessState::Ready
            && self.execution_target == ReadinessState::Ready);
        provider_ready && mode_ready && execution_ready
    }

    /// Whether this provider can satisfy the complete current role contract.
    /// Provider identity never grants a missing execution capability.
    pub fn supports_role(&self, role: HarnessRole) -> bool {
        if self.validate().is_err() {
            return false;
        }
        let role_ready = self
            .roles
            .iter()
            .find(|readiness| readiness.role == role)
            .is_some_and(|readiness| readiness.available == ReadinessState::Ready);
        let target_ready =
            !role.requires_execution_target() || self.execution_target == ReadinessState::Ready;
        self.supports_mode(role.harness_mode()) && role_ready && target_ready
    }

    /// Whether Planning can run from app-supplied context when no repository
    /// execution target is available. This does not enable repository reads.
    pub fn supports_context_only_planning(&self) -> bool {
        if self.validate().is_err() || self.execution_target == ReadinessState::Ready {
            return false;
        }
        self.supports_context_only_help()
    }

    /// Provider capability for an explicitly tool-free help request. Unlike
    /// the no-target planning fallback, this remains useful when an execution
    /// target exists. It grants no role or repository execution capability.
    pub fn supports_context_only_help(&self) -> bool {
        if self.validate().is_err() {
            return false;
        }
        let provider_ready = self.cli_binary == ReadinessState::Ready
            && self
                .cli_version
                .as_ref()
                .is_some_and(|version| !version.trim().is_empty())
            && self.authentication == ReadinessState::Ready
            && self.model_catalog == ReadinessState::Ready
            && self.scriptable_invocation == ReadinessState::Ready;
        let planning_ready = self.modes.iter().any(|readiness| {
            readiness.mode == crate::harness::ExecutionMode::Planning
                && readiness.available == ReadinessState::Ready
        });
        provider_ready && planning_ready
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported provider readiness schema"
        );
        anyhow::ensure!(
            !self.provider_id.is_empty()
                && self.provider_id.len() <= 64
                && self
                    .provider_id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "provider ID is invalid"
        );
        super::ids::validate_identifier(&self.probe_id)?;
        anyhow::ensure!(
            self.probe_cancellation_generation > 0 && self.probe_sequence > 0,
            "provider probe generation and sequence must be positive"
        );
        let mut modes = std::collections::HashSet::new();
        anyhow::ensure!(
            self.modes
                .iter()
                .all(|readiness| modes.insert(readiness.mode)),
            "provider readiness repeats an execution mode"
        );
        anyhow::ensure!(
            self.modes.len() == crate::harness::ExecutionMode::ALL.len()
                && crate::harness::ExecutionMode::ALL
                    .iter()
                    .all(|mode| modes.contains(mode)),
            "provider readiness must report every execution mode"
        );
        let mut roles = std::collections::HashSet::new();
        anyhow::ensure!(
            self.roles
                .iter()
                .all(|readiness| roles.insert(readiness.role)),
            "provider readiness repeats a role"
        );
        anyhow::ensure!(
            self.roles.len() == HarnessRole::ALL.len()
                && HarnessRole::ALL.iter().all(|role| roles.contains(role)),
            "provider readiness must report every harness role"
        );
        Ok(())
    }
}
