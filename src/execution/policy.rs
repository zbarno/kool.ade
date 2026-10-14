use crate::execution::{
    contract::{ExecutionModeV1, NetworkAccessV1, ResourceBudgetV1},
    errors::ExecutionErrorKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod accessors;
mod fingerprint;
mod restrict;
mod validation;
pub use fingerprint::policy_hash;
use validation::{validate_environment_allowlist, validate_service_ports, validate_write_scope};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemGrantV1 {
    WorkspaceRead,
    WorkspaceWrite,
    WorkspaceScopedWrite,
    ArtifactWrite,
}

/// Limits writable staging and synchronization to one task-clone directory.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWriteScopeV1 {
    pub root_relative: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApprovedRecipeCommandV1 {
    pub approval_ref: String,
    pub executable_id: String,
    pub argv: Vec<String>,
    pub working_directory_relative: String,
    pub allowed_environment_names: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAuthorityV1 {
    Application,
}

/// Immutable-by-interface policy. Deserialized worker copies must be validated before use.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicyV1 {
    schema_version: u16,
    authority: PolicyAuthorityV1,
    mode: ExecutionModeV1,
    filesystem: BTreeSet<FilesystemGrantV1>,
    #[serde(default)]
    workspace_write_scope: Option<WorkspaceWriteScopeV1>,
    #[serde(default)]
    approved_recipe_command: Option<ApprovedRecipeCommandV1>,
    network_access: NetworkAccessV1,
    network_service_ports: BTreeSet<u16>,
    environment_names: BTreeSet<String>,
    command_capabilities: BTreeSet<String>,
    tool_capabilities: BTreeSet<String>,
    resource_limits: ResourceBudgetV1,
    run_as_non_root: bool,
    host_secrets: bool,
    privileged: bool,
}

impl ExecutionPolicyV1 {
    pub fn for_mode(
        mode: ExecutionModeV1,
        resource_limits: ResourceBudgetV1,
        allowed_ports: BTreeSet<u16>,
    ) -> anyhow::Result<Self> {
        resource_limits.validate()?;
        anyhow::ensure!(
            mode == ExecutionModeV1::RuntimeQa || allowed_ports.is_empty(),
            "only runtime QA may request service ports"
        );
        validate_service_ports(&allowed_ports)?;
        let filesystem = match mode {
            ExecutionModeV1::PlanningReadOnly => [FilesystemGrantV1::WorkspaceRead].into(),
            ExecutionModeV1::Implementation => [
                FilesystemGrantV1::WorkspaceRead,
                FilesystemGrantV1::WorkspaceWrite,
                FilesystemGrantV1::ArtifactWrite,
            ]
            .into(),
            ExecutionModeV1::Verification => [
                FilesystemGrantV1::WorkspaceRead,
                FilesystemGrantV1::WorkspaceWrite,
                FilesystemGrantV1::WorkspaceScopedWrite,
                FilesystemGrantV1::ArtifactWrite,
            ]
            .into(),
            ExecutionModeV1::RuntimeQa => [
                FilesystemGrantV1::WorkspaceRead,
                FilesystemGrantV1::ArtifactWrite,
            ]
            .into(),
        };
        let commands: &[&str] = match mode {
            ExecutionModeV1::PlanningReadOnly => &["cat", "git", "rg", "sh"],
            ExecutionModeV1::Implementation => &["bash", "git", "rg", "sh"],
            ExecutionModeV1::Verification => &[
                "bash", "cargo", "git", "node", "npm", "python", "pytest", "sh",
            ],
            ExecutionModeV1::RuntimeQa => &["browser", "node", "playwright", "sh"],
        };
        let command_capabilities = commands
            .iter()
            .map(|command| (*command).to_owned())
            .collect();
        let tools: &[&str] = match mode {
            ExecutionModeV1::PlanningReadOnly => &["read_repository"],
            ExecutionModeV1::Implementation => &[
                "read_repository",
                "write_repository",
                "request_dependency",
                "request_resource",
            ],
            ExecutionModeV1::Verification => {
                &["read_repository", "write_repository", "report_evidence"]
            }
            ExecutionModeV1::RuntimeQa => {
                &["read_repository", "capture_artifact", "report_evidence"]
            }
        };
        let tool_capabilities = tools.iter().map(|tool| (*tool).to_owned()).collect();
        Ok(Self {
            schema_version: 1,
            authority: PolicyAuthorityV1::Application,
            mode,
            filesystem,
            workspace_write_scope: None,
            approved_recipe_command: None,
            network_access: if allowed_ports.is_empty() {
                NetworkAccessV1::Disabled
            } else {
                NetworkAccessV1::RuntimeQaServicePorts
            },
            network_service_ports: allowed_ports,
            environment_names: ["CI", "LANG", "LC_ALL", "TZ"].map(str::to_owned).into(),
            command_capabilities,
            tool_capabilities,
            resource_limits,
            run_as_non_root: true,
            host_secrets: false,
            privileged: false,
        })
    }

    pub fn for_scoped_workspace_write(
        mode: ExecutionModeV1,
        resource_limits: ResourceBudgetV1,
        root_relative: String,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            mode == ExecutionModeV1::Verification,
            "scoped recipe writes require verification mode"
        );
        validate_write_scope(&root_relative)?;
        let mut policy = Self::for_mode(mode, resource_limits, BTreeSet::new())?;
        policy.filesystem.remove(&FilesystemGrantV1::WorkspaceWrite);
        policy
            .filesystem
            .insert(FilesystemGrantV1::WorkspaceScopedWrite);
        policy.workspace_write_scope = Some(WorkspaceWriteScopeV1 { root_relative });
        Ok(policy)
    }

    pub(crate) fn without_scoped_workspace_write(mut self) -> Self {
        self.filesystem
            .remove(&FilesystemGrantV1::WorkspaceScopedWrite);
        self
    }

    pub fn for_approved_recipe(
        resource_limits: ResourceBudgetV1,
        root_relative: String,
        approval_ref: String,
        executable_id: String,
        argv: Vec<String>,
        working_directory_relative: String,
        allowed_environment_names: BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            approval_ref.starts_with("user-approval:") && approval_ref.len() <= 2048,
            "recipe command requires an explicit user approval receipt"
        );
        anyhow::ensure!(
            !matches!(
                executable_id.as_str(),
                "sh" | "bash" | "pwsh" | "powershell"
            ),
            "approved recipes cannot select a command interpreter"
        );
        anyhow::ensure!(
            !executable_id.is_empty()
                && executable_id.len() <= 128
                && executable_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')),
            "approved recipe executable is invalid"
        );
        anyhow::ensure!(
            !argv.is_empty()
                && argv.len() <= 128
                && argv
                    .iter()
                    .all(|arg| arg.len() <= 4096 && !arg.contains('\0')),
            "approved recipe arguments are invalid"
        );
        let mut policy = Self::for_scoped_workspace_write(
            ExecutionModeV1::Verification,
            resource_limits,
            root_relative,
        )?;
        policy.command_capabilities.insert(executable_id.clone());
        policy.environment_names = allowed_environment_names.clone();
        validate_environment_allowlist(&policy.environment_names)?;
        policy.approved_recipe_command = Some(ApprovedRecipeCommandV1 {
            approval_ref,
            executable_id,
            argv,
            working_directory_relative,
            allowed_environment_names,
        });
        Ok(policy)
    }

    /// Intersect grants and lower limits. The returned policy cannot widen either input.
    pub fn failure_kind_for_invalid(&self) -> ExecutionErrorKind {
        ExecutionErrorKind::PolicyDenied
    }
}
