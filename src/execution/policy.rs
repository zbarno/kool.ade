use crate::execution::{
    contract::{ExecutionModeV1, NetworkAccessV1, ResourceBudgetV1},
    errors::ExecutionErrorKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod validation;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemGrantV1 {
    WorkspaceRead,
    WorkspaceWrite,
    ArtifactWrite,
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
            ExecutionModeV1::PlanningReadOnly => &["cat", "git", "rg"],
            ExecutionModeV1::Implementation => &["bash", "git", "rg"],
            ExecutionModeV1::Verification => &["bash", "cargo", "git", "node", "npm", "pytest"],
            ExecutionModeV1::RuntimeQa => &["browser", "node", "playwright"],
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

    pub fn mode(&self) -> ExecutionModeV1 {
        self.mode
    }

    pub fn command_capabilities(&self) -> &BTreeSet<String> {
        &self.command_capabilities
    }

    pub fn filesystem_grants(&self) -> &BTreeSet<FilesystemGrantV1> {
        &self.filesystem
    }

    pub fn network_service_ports(&self) -> &BTreeSet<u16> {
        &self.network_service_ports
    }

    pub fn tool_capabilities(&self) -> &BTreeSet<String> {
        &self.tool_capabilities
    }

    pub fn resource_limits(&self) -> &ResourceBudgetV1 {
        &self.resource_limits
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported execution policy schema"
        );
        anyhow::ensure!(
            self.authority == PolicyAuthorityV1::Application,
            "policy authority is invalid"
        );
        anyhow::ensure!(
            self.run_as_non_root,
            "execution policy must require a non-root user"
        );
        anyhow::ensure!(
            !self.host_secrets,
            "execution policy cannot grant host secrets"
        );
        anyhow::ensure!(!self.privileged, "privileged execution is prohibited");
        self.resource_limits.validate()?;
        anyhow::ensure!(
            self.mode == ExecutionModeV1::RuntimeQa || self.network_service_ports.is_empty(),
            "only runtime QA may allow service ports"
        );
        anyhow::ensure!(
            match self.network_access {
                NetworkAccessV1::Disabled => self.network_service_ports.is_empty(),
                NetworkAccessV1::RuntimeQaServicePorts => {
                    self.mode == ExecutionModeV1::RuntimeQa
                        && !self.network_service_ports.is_empty()
                }
            },
            "network access mode and service ports disagree"
        );
        anyhow::ensure!(
            self.network_service_ports.iter().all(|port| *port != 0),
            "network service port zero is invalid"
        );
        validate_service_ports(&self.network_service_ports)?;
        let ceiling = Self::mode_ceiling(
            self.mode,
            &self.resource_limits,
            &self.network_service_ports,
        )?;
        anyhow::ensure!(
            self.filesystem.is_subset(&ceiling.filesystem)
                && self.environment_names.is_subset(&ceiling.environment_names)
                && self
                    .command_capabilities
                    .is_subset(&ceiling.command_capabilities)
                && self.tool_capabilities.is_subset(&ceiling.tool_capabilities),
            "policy grants exceed the mode ceiling"
        );
        Ok(())
    }

    fn mode_ceiling(
        mode: ExecutionModeV1,
        resource_limits: &ResourceBudgetV1,
        ports: &BTreeSet<u16>,
    ) -> anyhow::Result<Self> {
        match Self::for_mode(mode, resource_limits.clone(), ports.clone()) {
            Ok(policy) => Ok(policy),
            Err(error) if mode != ExecutionModeV1::RuntimeQa && !ports.is_empty() => Err(error),
            Err(error) => Err(error),
        }
    }

    /// Intersect grants and lower limits. The returned policy cannot widen either input.
    pub fn restrict_to(&self, ceiling: &Self) -> anyhow::Result<Self> {
        self.validate()?;
        ceiling.validate()?;
        anyhow::ensure!(
            self.mode == ceiling.mode,
            "cannot combine policies for different modes"
        );
        Ok(Self {
            schema_version: 1,
            authority: PolicyAuthorityV1::Application,
            mode: self.mode,
            filesystem: self
                .filesystem
                .intersection(&ceiling.filesystem)
                .copied()
                .collect(),
            network_access: if self
                .network_service_ports
                .intersection(&ceiling.network_service_ports)
                .next()
                .is_some()
            {
                NetworkAccessV1::RuntimeQaServicePorts
            } else {
                NetworkAccessV1::Disabled
            },
            network_service_ports: self
                .network_service_ports
                .intersection(&ceiling.network_service_ports)
                .copied()
                .collect(),
            environment_names: self
                .environment_names
                .intersection(&ceiling.environment_names)
                .cloned()
                .collect(),
            command_capabilities: self
                .command_capabilities
                .intersection(&ceiling.command_capabilities)
                .cloned()
                .collect(),
            tool_capabilities: self
                .tool_capabilities
                .intersection(&ceiling.tool_capabilities)
                .cloned()
                .collect(),
            resource_limits: self.resource_limits.restricted_by(&ceiling.resource_limits),
            run_as_non_root: true,
            host_secrets: false,
            privileged: false,
        })
    }

    pub fn failure_kind_for_invalid(&self) -> ExecutionErrorKind {
        ExecutionErrorKind::PolicyDenied
    }
}

fn validate_service_ports(ports: &BTreeSet<u16>) -> anyhow::Result<()> {
    anyhow::ensure!(ports.len() <= 16, "too many QA service ports");
    anyhow::ensure!(
        ports.iter().all(|port| (1024..=u16::MAX).contains(port)),
        "QA service ports must be unprivileged TCP ports"
    );
    Ok(())
}
