use super::ExecutionPolicyV1;
use crate::execution::{contract::ExecutionModeV1, contract::ExecutionRequestV1};

impl ExecutionPolicyV1 {
    pub fn validate_request(&self, request: &ExecutionRequestV1) -> anyhow::Result<()> {
        self.validate()?;
        anyhow::ensure!(
            self.mode == request.mode,
            "policy mode does not match request mode"
        );
        anyhow::ensure!(
            request
                .environment_allowlist
                .is_subset(&self.environment_names),
            "request environment exceeds policy"
        );
        anyhow::ensure!(
            request
                .allowed_service_ports
                .is_subset(&self.network_service_ports),
            "request service ports exceed policy"
        );
        if let Some(approved) = self.approved_recipe_command() {
            let mut exact_argv = Vec::with_capacity(approved.argv.len() + 1);
            exact_argv.push(approved.executable_id.clone());
            exact_argv.extend(approved.argv.iter().cloned());
            anyhow::ensure!(
                request.argv == exact_argv
                    && request.working_directory_relative == approved.working_directory_relative
                    && request.environment_allowlist == approved.allowed_environment_names,
                "execution request differs from the explicitly approved project recipe"
            );
        } else {
            anyhow::ensure!(
                !self
                    .filesystem_grants()
                    .contains(&crate::execution::policy::FilesystemGrantV1::WorkspaceScopedWrite),
                "scoped workspace writes require an approved exact recipe command"
            );
        }
        let requested = &request.resource_budget;
        let limit = &self.resource_limits;
        anyhow::ensure!(
            requested.cpu_millis <= limit.cpu_millis
                && requested.memory_bytes <= limit.memory_bytes
                && requested.process_count <= limit.process_count
                && requested.temporary_disk_bytes <= limit.temporary_disk_bytes
                && requested.output_bytes <= limit.output_bytes,
            "request resources exceed policy"
        );
        if request.mode != ExecutionModeV1::RuntimeQa {
            anyhow::ensure!(
                request.allowed_service_ports.is_empty(),
                "service ports are QA-only"
            );
        }
        Ok(())
    }
}

use super::*;

impl ExecutionPolicyV1 {
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
        validate_artifact_paths(&self.artifact_paths)?;
        anyhow::ensure!(
            self.artifact_paths.is_empty()
                || matches!(
                    self.mode,
                    ExecutionModeV1::Verification | ExecutionModeV1::RuntimeQa
                ),
            "only verification and runtime QA executions may collect project artifacts"
        );
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
                && self.command_capabilities.iter().all(|command| {
                    ceiling.command_capabilities.contains(command)
                        || self
                            .approved_recipe_command
                            .as_ref()
                            .is_some_and(|approved| approved.executable_id == *command)
                })
                && self.tool_capabilities.is_subset(&ceiling.tool_capabilities),
            "policy grants exceed the mode ceiling"
        );
        anyhow::ensure!(
            self.workspace_write_scope.as_ref().is_none_or(|_| {
                self.filesystem
                    .contains(&FilesystemGrantV1::WorkspaceScopedWrite)
            }),
            "scoped workspace write root requires its grant"
        );
        if let Some(scope) = &self.workspace_write_scope {
            anyhow::ensure!(
                self.mode == ExecutionModeV1::Verification,
                "scoped writes require verification mode"
            );
            validate_write_scope(&scope.root_relative)?;
            anyhow::ensure!(
                !self.filesystem.contains(&FilesystemGrantV1::WorkspaceWrite),
                "scoped and whole-workspace write grants cannot be combined"
            );
        }
        if let Some(command) = &self.approved_recipe_command {
            anyhow::ensure!(
                self.workspace_write_scope.is_some()
                    && self
                        .filesystem
                        .contains(&FilesystemGrantV1::WorkspaceScopedWrite)
                    && command.approval_ref.starts_with("user-approval:")
                    && command.approval_ref.len() <= 2048
                    && command.executable_id.len() <= 128
                    && self.command_capabilities.contains(&command.executable_id)
                    && command.argv.len() <= 128
                    && command
                        .argv
                        .iter()
                        .all(|arg| arg.len() <= 4096 && !arg.contains('\0'))
                    && command.allowed_environment_names == self.environment_names,
                "approved recipe command grant is invalid"
            );
            validate_recipe_working_directory(&command.working_directory_relative)?;
        }
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
}

pub(super) fn validate_write_scope(value: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(value);
    if value == "." {
        return Ok(());
    }
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 1024
            && !value.contains('\\')
            && !value.contains('\0')
            && path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
        "workspace write scope must be a canonical relative directory"
    );
    anyhow::ensure!(
        !path
            .components()
            .any(|component| component.as_os_str() == ".git"),
        "workspace write scope cannot include Git metadata"
    );
    Ok(())
}

fn validate_artifact_paths(paths: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(paths.len() <= 128, "too many requested artifact paths");
    let mut unique = std::collections::BTreeSet::new();
    for value in paths {
        let path = std::path::Path::new(value);
        anyhow::ensure!(
            !value.is_empty()
                && value.len() <= 1024
                && !value.contains('\\')
                && !value.chars().any(char::is_control)
                && value
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
                && !path
                    .components()
                    .any(|component| component.as_os_str() == ".git"),
            "artifact paths must be normalized workspace-relative files"
        );
        anyhow::ensure!(unique.insert(value), "artifact paths must be unique");
    }
    Ok(())
}

pub(super) fn validate_recipe_working_directory(value: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(value);
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 1024
            && !value.contains('\\')
            && !value.contains('\0')
            && path.components().all(|component| matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
        "approved recipe working directory escapes its target root"
    );
    Ok(())
}

pub(super) fn validate_service_ports(ports: &BTreeSet<u16>) -> anyhow::Result<()> {
    anyhow::ensure!(ports.len() <= 16, "too many QA service ports");
    anyhow::ensure!(
        ports.iter().all(|port| (1024..=u16::MAX).contains(port)),
        "QA service ports must be unprivileged TCP ports"
    );
    Ok(())
}

pub(super) fn validate_environment_allowlist(names: &BTreeSet<String>) -> anyhow::Result<()> {
    for name in names {
        anyhow::ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                && !name.as_bytes()[0].is_ascii_digit(),
            "environment allowlist contains an invalid variable name"
        );
        let upper = name.to_ascii_uppercase();
        anyhow::ensure!(
            ![
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PRIVATE_KEY",
                "CREDENTIAL",
                "SSH_",
                "AWS_",
                "GITHUB_"
            ]
            .iter()
            .any(|sensitive| upper.contains(sensitive)),
            "credential environment variables cannot be granted to an execution"
        );
    }
    Ok(())
}
