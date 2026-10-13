//! Durable, operator-local lifecycle records for container executions.
//!
//! Records live outside project clones and shared planning repositories so a
//! restart can reconcile an execution without publishing runtime metadata.

use serde::{Deserialize, Serialize};

pub(super) const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecordV1 {
    pub schema_version: u16,
    pub execution_id: String,
    #[serde(default)]
    pub request: Option<crate::execution::contract::ExecutionRequestV1>,
    pub project_id: String,
    pub task_id: String,
    pub target_id: String,
    /// Controller-selected worker identity; `target_id` remains the local
    /// worker-side transport target used for recovery and command control.
    #[serde(default)]
    pub controller_target_id: Option<String>,
    pub owner_instance_id: String,
    pub container_name: Option<String>,
    #[serde(default)]
    pub workspace_lease_id: Option<String>,
    pub workspace_source_sha: String,
    pub destination_sha: Option<String>,
    #[serde(default)]
    pub workspace_manifest: Option<Vec<ExecutionWorkspaceEntryV1>>,
    pub container_id: Option<String>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub command_started: bool,
    #[serde(default)]
    pub result: Option<crate::execution::contract::ExecutionResultV1>,
    pub image_digest: String,
    pub policy_version: String,
    pub lease_generation: u64,
    #[serde(default)]
    pub lease_expires_at_unix_ms: Option<u64>,
    pub attempt: u32,
    pub state: ExecutionRecordStateV1,
    pub event_cursor: u64,
    #[serde(default)]
    pub output_bytes: u64,
    pub started_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub ended_at_unix_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub artifacts: Vec<ExecutionArtifactV1>,
    pub cleanup: ExecutionCleanupStateV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionWorkspaceEntryV1 {
    pub relative_path: String,
    pub kind: ExecutionWorkspaceEntryKindV1,
    pub sha256: Option<String>,
    pub size_bytes: u64,
    pub mode: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionWorkspaceEntryKindV1 {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecordStateV1 {
    Preparing,
    Running,
    WaitingForDependencyApproval,
    WaitingForRuntime,
    CollectingResults,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionCleanupStateV1 {
    Pending,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionArtifactV1 {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

impl ExecutionRecordV1 {
    pub fn validate(&self) -> Result<(), String> {
        let uuid_fields = [&self.execution_id, &self.project_id, &self.task_id];
        if self.schema_version != SCHEMA_VERSION
            || uuid_fields.iter().any(|value| !is_canonical_uuid(value))
            || self.target_id.is_empty()
            || self.target_id.len() > 128
            || !is_canonical_uuid(&self.owner_instance_id)
            || self.policy_version.is_empty()
            || self.policy_version.len() > 128
            || !is_sha256(&self.image_digest)
            || !is_git_sha(&self.workspace_source_sha)
            || self
                .destination_sha
                .as_deref()
                .is_some_and(|sha| !is_git_sha(sha))
            || self.lease_generation == 0
            || self
                .lease_expires_at_unix_ms
                .is_some_and(|expires| expires < self.started_at_unix_ms)
            || self.attempt == 0
            || self.output_bytes > 1024 * 1024 * 1024
            || self.started_at_unix_ms == 0
            || self.updated_at_unix_ms < self.started_at_unix_ms
            || self
                .ended_at_unix_ms
                .is_some_and(|end| end < self.updated_at_unix_ms)
        {
            return Err("execution record identity or lifecycle fields are invalid".into());
        }
        if self.container_id.as_deref().is_some_and(|id| {
            id.is_empty() || id.len() > 128 || id.bytes().any(|byte| byte.is_ascii_control())
        }) {
            return Err("execution record container identity is invalid".into());
        }
        if self
            .controller_target_id
            .as_deref()
            .is_some_and(|target| target != "local" && !is_canonical_uuid(target))
        {
            return Err("execution record controller target identity is invalid".into());
        }
        if self.command_id.as_deref().is_some_and(|id| {
            id.is_empty()
                || id.len() > 128
                || id
                    .bytes()
                    .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-'))
        }) {
            return Err("execution record command identity is invalid".into());
        }
        if self
            .workspace_lease_id
            .as_deref()
            .is_some_and(|lease| !is_canonical_uuid(lease))
        {
            return Err("execution record workspace lease identity is invalid".into());
        }
        if self.result.as_ref().is_some_and(|result| {
            result.execution_id != self.execution_id
                || result.validate().is_err()
                || result.exit_code != self.exit_code
        }) {
            return Err("execution record result does not match its identity".into());
        }
        if let Some(request) = &self.request
            && (request.execution_id != self.execution_id
                || request.task_id != self.task_id
                || request.project_id != self.project_id
                || request.workspace.source_sha != self.workspace_source_sha
                || request.target.target_id != self.target_id
                || request.controller_target_id != self.controller_target_id
                || request.image_digest != self.image_digest
                || request.cancellation_generation != self.lease_generation
                || u32::try_from(request.request_sequence).unwrap_or(u32::MAX) != self.attempt
                || request.validate_at(self.started_at_unix_ms).is_err()
                || self.output_bytes > request.resource_budget.output_bytes)
        {
            return Err("execution record request does not match its identity".into());
        }
        if self.container_name.as_deref().is_some_and(|name| {
            !name.starts_with("koolade-exec-")
                || name.len() > 96
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        }) {
            return Err("execution record container name is invalid".into());
        }
        let terminal = matches!(
            self.state,
            ExecutionRecordStateV1::Succeeded
                | ExecutionRecordStateV1::Failed
                | ExecutionRecordStateV1::Cancelled
                | ExecutionRecordStateV1::Interrupted
        );
        let artifact_bytes = self
            .artifacts
            .iter()
            .try_fold(0_u64, |sum, artifact| sum.checked_add(artifact.size_bytes));
        if terminal != self.ended_at_unix_ms.is_some()
            || self.artifacts.len() > 256
            || !artifact_bytes.is_some_and(|bytes| bytes <= 1024 * 1024 * 1024)
            || (self.cleanup == ExecutionCleanupStateV1::Complete && !terminal)
        {
            return Err("execution record result or cleanup fields are invalid".into());
        }
        let mut artifact_paths = std::collections::BTreeSet::new();
        for artifact in &self.artifacts {
            let path = std::path::Path::new(&artifact.relative_path);
            if artifact.relative_path.is_empty()
                || path.is_absolute()
                || artifact.relative_path.contains('\\')
                || artifact.relative_path.chars().any(char::is_control)
                || !path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                || !is_sha256(&artifact.sha256)
                || !artifact_paths.insert(&artifact.relative_path)
            {
                return Err("execution record artifact manifest is invalid".into());
            }
        }
        if let Some(entries) = &self.workspace_manifest {
            if entries.len() > 100_000
                || entries
                    .iter()
                    .try_fold(0_u64, |sum, entry| sum.checked_add(entry.size_bytes))
                    .is_none_or(|bytes| bytes > 512 * 1024 * 1024)
            {
                return Err("execution workspace baseline exceeds its limit".into());
            }
            let path_bytes = entries.iter().try_fold(0_usize, |sum, entry| {
                sum.checked_add(entry.relative_path.len())
            });
            if path_bytes.is_none_or(|bytes| bytes > 16 * 1024 * 1024) {
                return Err("execution workspace baseline paths exceed their limit".into());
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut kinds = std::collections::BTreeMap::new();
            for entry in entries {
                let path = std::path::Path::new(&entry.relative_path);
                if entry.relative_path.is_empty()
                    || entry.relative_path.len() > 4096
                    || path.is_absolute()
                    || entry.relative_path.contains('\\')
                    || entry.relative_path.chars().any(char::is_control)
                    || !path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                    || entry.relative_path == ".git"
                    || entry.relative_path.starts_with(".git/")
                    || !seen.insert(&entry.relative_path)
                    || entry.mode > 0o777
                    || match entry.kind {
                        ExecutionWorkspaceEntryKindV1::Directory => {
                            entry.sha256.is_some() || entry.size_bytes != 0
                        }
                        ExecutionWorkspaceEntryKindV1::File => {
                            !entry.sha256.as_deref().is_some_and(|sha| {
                                sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                            })
                        }
                    }
                {
                    return Err("execution workspace baseline entry is invalid".into());
                }
                kinds.insert(entry.relative_path.as_str(), entry.kind);
            }
            for entry in entries {
                let path = std::path::Path::new(&entry.relative_path);
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                    && kinds.get(parent.to_str().unwrap_or_default())
                        != Some(&ExecutionWorkspaceEntryKindV1::Directory)
                {
                    return Err("execution workspace baseline parent directory is missing".into());
                }
            }
        }
        Ok(())
    }
}

pub(super) fn is_canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

pub(super) fn is_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
