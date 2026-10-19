//! Container configuration built only from validated application policy.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use bollard::models::{ContainerCreateBody, HostConfig};

use super::images::{ImageLock, RuntimeProfile, SelectedImage};
use crate::execution::{
    contract::ExecutionRequestV1,
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(crate) mod staging;
pub(crate) use staging::WorkspaceSnapshot;
mod private_files;
mod workspace_identity;

pub const WORKSPACE_TARGET: &str = "/workspace";
pub const OWNER_LABEL: &str = "com.koolade.owner";
pub const EXECUTION_LABEL: &str = "com.koolade.execution";
pub const TASK_LABEL: &str = "com.koolade.task";
pub const WORKSPACE_LABEL: &str = "com.koolade.workspace";
pub const POLICY_LABEL: &str = "com.koolade.policy";
pub const LEASE_LABEL: &str = "com.koolade.lease-unix-ms";
pub(crate) const PRIVATE_FILES_TARGET: &str = "/koolade-private-files";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedWorkspace {
    root: PathBuf,
    git_metadata: PathBuf,
    root_identity: workspace_identity::WorkspaceObjectId,
    git_identity: workspace_identity::WorkspaceObjectId,
}

impl ValidatedWorkspace {
    /// Resolve and revalidate an application-owned task clone immediately before mount creation.
    /// Callers must obtain the root from the task-clone registry, never from request data.
    pub fn inspect(root: &Path) -> Result<Self, ExecutionError> {
        let canonical = root
            .canonicalize()
            .map_err(|_| denied("Task clone is unavailable"))?;
        let metadata = std::fs::symlink_metadata(&canonical)
            .map_err(|_| denied("Task clone metadata could not be verified"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(denied("Task clone root is not a real directory"));
        }
        let root_identity = workspace_identity::from_metadata(&metadata)
            .ok_or_else(|| denied("Task clone identity cannot be verified on this platform"))?;
        let git_path = canonical.join(".git");
        let git_meta = std::fs::symlink_metadata(&git_path)
            .map_err(|_| denied("Task clone Git metadata is unavailable"))?;
        if !git_meta.is_dir() || git_meta.file_type().is_symlink() {
            return Err(denied("Task clone must contain a real .git directory"));
        }
        let git_identity = workspace_identity::from_metadata(&git_meta)
            .ok_or_else(|| denied("Task clone Git metadata identity cannot be verified"))?;
        Ok(Self {
            root: canonical,
            git_metadata: git_path,
            root_identity,
            git_identity,
        })
    }

    pub fn revalidate(&self) -> Result<(), ExecutionError> {
        let current = Self::inspect(&self.root)?;
        if current != *self {
            return Err(denied("Task clone path changed before container launch"));
        }
        Ok(())
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub(super) fn git_metadata(&self) -> &Path {
        &self.git_metadata
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxIdentity {
    pub owner_instance_id: String,
    pub policy_version: String,
    pub lease_expires_unix_ms: u64,
}

impl SandboxIdentity {
    pub fn validate(&self) -> Result<(), ExecutionError> {
        for value in [&self.owner_instance_id, &self.policy_version] {
            if value.is_empty() || value.len() > 128 || value.bytes().any(|b| b.is_ascii_control())
            {
                return Err(denied("Sandbox identity metadata is invalid"));
            }
        }
        if self.lease_expires_unix_ms == 0 {
            return Err(denied("Sandbox lease is invalid"));
        }
        Ok(())
    }
}

/// Fixed Docker create body. There are deliberately no user-supplied mount, namespace, or security fields.
pub fn create_body(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
) -> Result<ContainerCreateBody, ExecutionError> {
    create_body_with_derived_approval(request, workspace, identity, image, None)
}

pub(super) fn create_body_with_derived_approval(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
    derived_approval: Option<&super::backend::PreparedDerivedImage>,
) -> Result<ContainerCreateBody, ExecutionError> {
    create_body_with_private_files(request, workspace, identity, image, derived_approval, None)
}

pub(super) fn create_body_with_private_files(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
    derived_approval: Option<&super::backend::PreparedDerivedImage>,
    private_files_root: Option<&Path>,
) -> Result<ContainerCreateBody, ExecutionError> {
    request
        .validate_at(now_unix_ms())
        .map_err(|_| denied("Execution request or policy is invalid"))?;
    identity.validate()?;
    workspace.revalidate()?;
    let approved = if let Some(approval) = derived_approval {
        approval.matches(image)
    } else {
        let lock = ImageLock::bundled()
            .map_err(|_| denied("Approved executor image manifest is invalid"))?;
        let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
        let approved = lock
            .select(image.profile, architecture)
            .map_err(|_| denied("Sandbox image profile is not approved"))?;
        approved == *image
    };
    if !approved || image.digest != request.image_digest {
        return Err(denied(
            "Sandbox image does not match the approved request digest",
        ));
    }
    let labels = HashMap::from([
        (OWNER_LABEL.into(), identity.owner_instance_id.clone()),
        (EXECUTION_LABEL.into(), request.execution_id.clone()),
        (TASK_LABEL.into(), request.task_id.clone()),
        (
            WORKSPACE_LABEL.into(),
            format!(
                "{}:{}",
                request.workspace.project_id, request.workspace.task_id
            ),
        ),
        (POLICY_LABEL.into(), identity.policy_version.clone()),
        (
            LEASE_LABEL.into(),
            identity.lease_expires_unix_ms.to_string(),
        ),
    ]);
    let mut mounts = if request.policy.workspace_write_scope().is_none() {
        vec![private_files::bind_mount(
            &workspace.git_metadata,
            "/workspace/.git",
            true,
            false,
        )?]
    } else {
        Vec::new()
    };
    if let Some(root) = private_files_root {
        private_files::validate_root(root)?;
        mounts.push(private_files::bind_mount(
            root,
            PRIVATE_FILES_TARGET,
            true,
            true,
        )?);
    }
    let temporary_bytes = request.resource_budget.temporary_disk_bytes / 4;
    let mut tmpfs = HashMap::new();
    // The staged snapshot is materialized after container start. Planning mode
    // is sealed before any user command runs, while the image rootfs and .git
    // mount remain read-only throughout setup.
    let workspace_mode = "1777";
    let workspace_access = "rw";
    let workspace_exec =
        if request.mode == crate::execution::contract::ExecutionModeV1::PlanningReadOnly {
            "noexec"
        } else {
            "exec"
        };
    tmpfs.insert(
        WORKSPACE_TARGET.into(),
        format!("{workspace_access},{workspace_exec},nosuid,nodev,size={temporary_bytes},mode={workspace_mode}"),
    );
    for target in ["/tmp", "/run", "/home/executor"] {
        tmpfs.insert(
            target.to_owned(),
            format!("rw,nosuid,nodev,noexec,size={temporary_bytes}"),
        );
    }
    let mut environment = vec![
        "HOME=/home/executor".into(),
        "CI=true".into(),
        "LANG=C".into(),
        "TZ=UTC".into(),
    ];
    if image.profile == RuntimeProfile::Rust {
        // The approved Rust image sets CARGO_HOME to /usr/local/cargo. Broker
        // snapshots are staged under the executor's private home instead.
        environment.push("CARGO_HOME=/home/executor/.cargo".into());
    }
    if image.profile == RuntimeProfile::Go {
        // /tmp intentionally stays noexec. Go creates temporary test and run
        // binaries here and removes them when the command finishes.
        environment.push("GOTMPDIR=/workspace".into());
    }
    // `/workspace/.git` is protected by a nested read-only mount, never by command filtering.
    Ok(ContainerCreateBody {
        user: Some("10001:10001".into()),
        env: Some(environment),
        attach_stdin: Some(false),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        tty: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        image: Some(image.reference.clone()),
        working_dir: Some(WORKSPACE_TARGET.into()),
        entrypoint: Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            "mkdir -p /home/executor/.npm /home/executor/.cargo && while :; do sleep 3600; done"
                .into(),
            format!("koolade-keeper-{}", request.execution_id),
        ]),
        labels: Some(labels),
        host_config: Some(HostConfig {
            privileged: Some(false),
            readonly_rootfs: Some(true),
            network_mode: Some("none".into()),
            ipc_mode: Some("private".into()),
            // Docker's default PID and user namespaces are isolated. Omit
            // explicit mode strings because rootless Engine rejects `private`.
            userns_mode: None,
            cap_drop: Some(vec!["ALL".into()]),
            cap_add: Some(Vec::new()),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            memory: Some(
                i64::try_from(request.resource_budget.memory_bytes)
                    .map_err(|_| denied("Memory limit is out of range"))?,
            ),
            memory_swap: Some(
                i64::try_from(request.resource_budget.memory_bytes)
                    .map_err(|_| denied("Memory limit is out of range"))?,
            ),
            cpu_period: Some(100_000),
            cpu_quota: Some(i64::from(request.resource_budget.cpu_millis) * 100),
            pids_limit: Some(i64::from(request.resource_budget.process_count)),
            init: Some(true),
            auto_remove: Some(false),
            mounts: Some(mounts),
            tmpfs: Some(tmpfs),
            storage_opt: Some(HashMap::from([(
                "size".into(),
                format!("{}", request.resource_budget.temporary_disk_bytes),
            )])),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn denied(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "sandbox/tests.rs"]
mod tests;
