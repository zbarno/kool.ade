use std::{collections::HashMap, path::Path};

use bollard::models::{ContainerCreateBody, HostConfig};

use super::super::{
    backend::PreparedDerivedImage,
    images::{ImageLock, SelectedImage},
};
use super::{
    EXECUTION_LABEL, LEASE_LABEL, OWNER_LABEL, POLICY_LABEL, PRIVATE_FILES_TARGET, SandboxIdentity,
    TASK_LABEL, ValidatedWorkspace, WORKSPACE_LABEL, WORKSPACE_TARGET, denied, now_unix_ms,
    private_files,
};
use crate::execution::{contract::ExecutionRequestV1, errors::ExecutionError};

/// Fixed Docker create body. There are deliberately no user-supplied mount, namespace, or security fields.
pub fn create_body(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
) -> Result<ContainerCreateBody, ExecutionError> {
    create_body_with_derived_approval(request, workspace, identity, image, None)
}

pub(crate) fn create_body_with_derived_approval(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
    derived_approval: Option<&PreparedDerivedImage>,
) -> Result<ContainerCreateBody, ExecutionError> {
    create_body_with_private_files(request, workspace, identity, image, derived_approval, None)
}

pub(crate) fn create_body_with_private_files(
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    identity: &SandboxIdentity,
    image: &SelectedImage,
    derived_approval: Option<&PreparedDerivedImage>,
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
    let environment = super::environment::variables(request, image);
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
            "mkdir -p /home/executor/.npm /home/executor/.cargo /home/executor/.cache/koolade/pip/wheels /home/executor/go && while :; do sleep 3600; done".into(),
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
