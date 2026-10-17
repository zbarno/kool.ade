use super::ids::{ExecutionTarget, WorkspaceRef};
use crate::execution::{errors::ExecutionErrorKind, policy::ExecutionPolicyV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod argv;
mod cache_ref;
mod validation;
pub use argv::MAX_SCRIPT_BYTES;
use argv::validate_argv;
pub use cache_ref::{DependencyCacheEcosystemV1, DependencyCacheRefV1};
use validation::{is_digest, validate_environment_names, validate_working_directory};

/// Hard ceiling that keeps task deadlines and recovery leases bounded.
pub const MAX_EXECUTION_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionModeV1 {
    PlanningReadOnly,
    Implementation,
    Verification,
    RuntimeQa,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccessV1 {
    Disabled,
    RuntimeQaServicePorts,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudgetV1 {
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub process_count: u32,
    pub temporary_disk_bytes: u64,
    pub output_bytes: u64,
}

impl Default for ResourceBudgetV1 {
    fn default() -> Self {
        Self {
            cpu_millis: 4_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            process_count: 256,
            temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ResourceBudgetV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=16_000).contains(&self.cpu_millis),
            "CPU budget is outside supported limits"
        );
        anyhow::ensure!(
            (16 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&self.memory_bytes),
            "memory budget is outside supported limits"
        );
        anyhow::ensure!(
            (1..=4096).contains(&self.process_count),
            "process budget is outside supported limits"
        );
        anyhow::ensure!(
            (16 * 1024 * 1024..=100 * 1024 * 1024 * 1024).contains(&self.temporary_disk_bytes),
            "temporary disk budget is outside supported limits"
        );
        anyhow::ensure!(
            (1024..=1024 * 1024 * 1024).contains(&self.output_bytes),
            "output budget is outside supported limits"
        );
        Ok(())
    }

    pub fn restricted_by(&self, ceiling: &Self) -> Self {
        Self {
            cpu_millis: self.cpu_millis.min(ceiling.cpu_millis),
            memory_bytes: self.memory_bytes.min(ceiling.memory_bytes),
            process_count: self.process_count.min(ceiling.process_count),
            temporary_disk_bytes: self.temporary_disk_bytes.min(ceiling.temporary_disk_bytes),
            output_bytes: self.output_bytes.min(ceiling.output_bytes),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequestV1 {
    pub schema_version: u16,
    pub execution_id: String,
    /// App-owned authorization generation captured when this session starts.
    /// It is persisted with the request and checked again before result apply.
    pub authorization_generation: String,
    pub project_id: String,
    pub task_id: String,
    pub target: ExecutionTarget,
    /// Controller-selected target retained across a remote worker's local
    /// transport target. This is audit metadata only and grants no authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_target_id: Option<String>,
    pub workspace: WorkspaceRef,
    /// Selected destination branch head observed when execution was authorized.
    /// This is provenance only; it grants no repository or publication authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_sha: Option<String>,
    pub mode: ExecutionModeV1,
    pub image_digest: String,
    pub argv: Vec<String>,
    /// Relative directory inside the isolated task workspace for this command.
    #[serde(default = "workspace_root")]
    pub working_directory_relative: String,
    /// Optional bounded script sent over container stdin (never embedded in argv).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    /// Names only: values are resolved from target-local, non-secret defaults.
    pub environment_allowlist: BTreeSet<String>,
    pub resource_budget: ResourceBudgetV1,
    pub deadline_unix_ms: u64,
    pub allowed_service_ports: BTreeSet<u16>,
    pub broker_capability_ids: BTreeSet<String>,
    /// Exact app-authorized cache snapshots staged by the controller. Bytes
    /// are transferred separately; the execution request contains refs only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_cache_refs: Vec<DependencyCacheRefV1>,
    pub cancellation_generation: u64,
    pub request_sequence: u64,
    pub policy: ExecutionPolicyV1,
}

impl ExecutionRequestV1 {
    pub fn validate_at(&self, now_unix_ms: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported execution request schema"
        );
        super::ids::validate_identifier(&self.execution_id)?;
        super::ids::validate_identifier(&self.authorization_generation)?;
        super::ids::validate_identifier(&self.project_id)?;
        super::ids::validate_identifier(&self.task_id)?;
        self.target.validate()?;
        anyhow::ensure!(
            self.controller_target_id.as_deref().is_none_or(|target| {
                target == "local" || super::ids::validate_identifier(target).is_ok()
            }),
            "controller target identity is invalid"
        );
        self.workspace.validate()?;
        anyhow::ensure!(
            self.destination_sha
                .as_deref()
                .is_none_or(super::ids::is_valid_sha),
            "destination SHA is invalid"
        );
        validate_working_directory(&self.working_directory_relative)?;
        anyhow::ensure!(
            self.workspace.project_id == self.project_id,
            "workspace project ID mismatch"
        );
        anyhow::ensure!(
            self.workspace.task_id == self.task_id,
            "workspace task ID mismatch"
        );
        anyhow::ensure!(
            is_digest(&self.image_digest),
            "execution image must use a sha256 digest"
        );
        anyhow::ensure!(!self.argv.is_empty(), "execution argv cannot be empty");
        anyhow::ensure!(
            self.stdin.as_ref().is_none_or(|script| {
                script.len() <= MAX_SCRIPT_BYTES && !script.contains('\0')
            }),
            "execution script exceeds limits"
        );
        self.resource_budget.validate()?;
        anyhow::ensure!(
            self.deadline_unix_ms > now_unix_ms,
            "execution request deadline has expired"
        );
        anyhow::ensure!(
            self.deadline_unix_ms <= now_unix_ms.saturating_add(MAX_EXECUTION_DURATION_MS),
            "execution request deadline exceeds the maximum duration"
        );
        anyhow::ensure!(
            self.request_sequence > 0,
            "request sequence must be positive"
        );
        anyhow::ensure!(
            self.cancellation_generation > 0,
            "cancellation generation must be positive"
        );
        anyhow::ensure!(
            self.broker_capability_ids
                .iter()
                .all(|id| super::ids::validate_identifier(id).is_ok()),
            "broker capability ID is invalid"
        );
        validate_environment_names(&self.environment_allowlist)?;
        anyhow::ensure!(
            self.dependency_cache_refs.len() <= 8,
            "too many dependency cache references"
        );
        let mut cache_refs = BTreeSet::new();
        let mut cache_bytes = 0_u64;
        for cache_ref in &self.dependency_cache_refs {
            cache_ref.validate()?;
            anyhow::ensure!(
                cache_ref.task_id == self.task_id && cache_ref.project_id == self.project_id,
                "dependency cache scope does not match execution task"
            );
            anyhow::ensure!(
                cache_ref.target_id
                    == self
                        .controller_target_id
                        .as_deref()
                        .unwrap_or(&self.target.target_id),
                "dependency cache target does not match execution target"
            );
            anyhow::ensure!(
                cache_refs.insert(cache_ref.clone()),
                "duplicate dependency cache reference"
            );
            cache_bytes = cache_bytes.saturating_add(cache_ref.size_bytes);
        }
        anyhow::ensure!(
            cache_bytes <= 128 * 1024 * 1024,
            "dependency cache transfer exceeds the session limit"
        );
        validate_argv(&self.argv, self.stdin.is_some(), &self.policy)?;
        self.policy.validate_request(self)?;
        Ok(())
    }

    /// Worker-side check: reject a message if its policy would need narrowing
    /// to fit the worker's immutable policy ceiling.
    pub fn validate_under_policy(
        &self,
        now_unix_ms: u64,
        worker_ceiling: &ExecutionPolicyV1,
    ) -> anyhow::Result<()> {
        self.validate_at(now_unix_ms)?;
        let bounded = self.policy.restrict_to(worker_ceiling)?;
        anyhow::ensure!(
            bounded == self.policy,
            "request policy exceeds worker ceiling"
        );
        Ok(())
    }

    pub fn validate_control(
        &self,
        now_unix_ms: u64,
        current_generation: u64,
        cancelled: bool,
    ) -> Result<(), ExecutionErrorKind> {
        if cancelled || current_generation != self.cancellation_generation {
            return Err(ExecutionErrorKind::Cancelled);
        }
        if now_unix_ms >= self.deadline_unix_ms {
            return Err(ExecutionErrorKind::DeadlineExceeded);
        }
        Ok(())
    }
}

fn workspace_root() -> String {
    ".".into()
}

pub fn ensure_wire_compatible(version: u16) -> Result<(), ExecutionErrorKind> {
    if version == super::EXECUTION_WIRE_VERSION_V1 {
        Ok(())
    } else {
        Err(ExecutionErrorKind::WorkerIncompatible)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionWireMessageV1 {
    pub wire_version: u16,
    pub request: ExecutionRequestV1,
}

impl ExecutionWireMessageV1 {
    pub fn validate_at(&self, now_unix_ms: u64) -> anyhow::Result<()> {
        ensure_wire_compatible(self.wire_version)
            .map_err(|kind| anyhow::anyhow!("unsupported execution wire version: {kind:?}"))?;
        self.request.validate_at(now_unix_ms)
    }
}
