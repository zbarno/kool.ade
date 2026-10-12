use serde::{Deserialize, Serialize};

use super::request::ExecutionModeV1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessRole {
    Manager,
    Specification,
    TaskGeneration,
    Investigation,
    Implementation,
    QualityAssurance,
    Documentation,
    Reconciliation,
    DecisionExplanation,
}

impl HarnessRole {
    pub fn harness_mode(self) -> crate::harness::ExecutionMode {
        use crate::harness::ExecutionMode;
        match self {
            Self::Manager => ExecutionMode::ReadOnlyAnalysis,
            Self::Specification => ExecutionMode::Planning,
            Self::TaskGeneration => ExecutionMode::TaskGeneration,
            Self::Investigation => ExecutionMode::Investigation,
            Self::Implementation => ExecutionMode::Implementation,
            Self::QualityAssurance => ExecutionMode::ReadOnlyAnalysis,
            Self::Documentation => ExecutionMode::ReadOnlyAnalysis,
            Self::Reconciliation => ExecutionMode::Reconciliation,
            Self::DecisionExplanation => ExecutionMode::DecisionExplanation,
        }
    }

    /// Container tool authority for the role; provider selection cannot alter it.
    pub fn execution_mode(self) -> ExecutionModeV1 {
        match self {
            Self::Implementation => ExecutionModeV1::Implementation,
            Self::QualityAssurance => ExecutionModeV1::RuntimeQa,
            Self::Manager
            | Self::Specification
            | Self::TaskGeneration
            | Self::Investigation
            | Self::Documentation
            | Self::Reconciliation
            | Self::DecisionExplanation => ExecutionModeV1::PlanningReadOnly,
        }
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
pub struct ProviderReadinessV1 {
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
    pub roles: Vec<RoleReadinessV1>,
}

impl ProviderReadinessV1 {
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
        let mut roles = std::collections::BTreeSet::new();
        anyhow::ensure!(
            self.roles
                .iter()
                .all(|readiness| roles.insert(readiness.role as u8)),
            "provider readiness repeats a role"
        );
        Ok(())
    }
}
