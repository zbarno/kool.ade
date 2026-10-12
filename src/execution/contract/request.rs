use super::ids::{ExecutionTarget, WorkspaceRef};
use crate::execution::{errors::ExecutionErrorKind, policy::ExecutionPolicyV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    pub project_id: String,
    pub task_id: String,
    pub target: ExecutionTarget,
    pub workspace: WorkspaceRef,
    pub mode: ExecutionModeV1,
    pub image_digest: String,
    pub argv: Vec<String>,
    /// Names only: values are resolved from target-local, non-secret defaults.
    pub environment_allowlist: BTreeSet<String>,
    pub resource_budget: ResourceBudgetV1,
    pub deadline_unix_ms: u64,
    pub allowed_service_ports: BTreeSet<u16>,
    pub broker_capability_ids: BTreeSet<String>,
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
        super::ids::validate_identifier(&self.project_id)?;
        super::ids::validate_identifier(&self.task_id)?;
        self.target.validate()?;
        self.workspace.validate()?;
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
        self.resource_budget.validate()?;
        anyhow::ensure!(
            self.deadline_unix_ms > now_unix_ms,
            "execution request deadline has expired"
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
        validate_argv(&self.argv, &self.policy)?;
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

fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn validate_environment_names(names: &BTreeSet<String>) -> anyhow::Result<()> {
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

fn validate_argv(argv: &[String], policy: &ExecutionPolicyV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        argv.iter().all(|arg| !arg.contains('\0')),
        "argv contains a NUL byte"
    );
    let executable = argv[0].rsplit(['/', '\\']).next().unwrap_or(&argv[0]);
    anyhow::ensure!(
        policy.command_capabilities().contains(executable),
        "executable is outside the policy command capabilities"
    );
    anyhow::ensure!(
        !argv
            .iter()
            .skip(1)
            .any(|argument| is_shell_command_flag(executable, argument)),
        "shell command strings are not valid execution argv"
    );
    anyhow::ensure!(
        argv[0] == executable,
        "execution binary must be selected by its image command name"
    );
    for argument in argv {
        anyhow::ensure!(
            !contains_host_path(argument),
            "argv cannot contain controller host paths"
        );
    }
    Ok(())
}

fn is_shell_command_flag(executable: &str, argument: &str) -> bool {
    match executable {
        "sh" | "bash" => {
            argument == "-c"
                || argument == "--command"
                || argument.starts_with("--command=")
                || (argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument.as_bytes()[1..].contains(&b'c'))
        }
        "pwsh" | "powershell" => {
            let flag = argument.to_ascii_lowercase();
            matches!(flag.as_str(), "-c" | "-command" | "-encodedcommand" | "-e")
                || flag.starts_with("-command:")
                || flag.starts_with("-encodedcommand:")
        }
        _ => false,
    }
}

fn contains_host_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with('/')
        || lower.starts_with("~/")
        || lower.starts_with("~\\")
        || lower.contains("=/")
        || lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("\\users\\")
        || (lower.len() >= 3
            && lower.as_bytes()[0].is_ascii_alphabetic()
            && lower.as_bytes()[1] == b':'
            && matches!(lower.as_bytes()[2], b'/' | b'\\'))
}
