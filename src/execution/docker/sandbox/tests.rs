use std::{collections::BTreeSet, fs, path::PathBuf};

use bollard::models::MountType;

use crate::execution::docker::{
    images::{ImageLock, RuntimeProfile, SelectedImage},
    sandbox::{LEASE_LABEL, SandboxIdentity, ValidatedWorkspace, create_body},
};
use crate::execution::{
    contract::{
        ExecutionModeV1, ExecutionRequestV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    policy::ExecutionPolicyV1,
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("koolade-sandbox-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join(".git")).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(mode: ExecutionModeV1) -> ExecutionRequestV1 {
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let budget = ResourceBudgetV1 {
        cpu_millis: 2000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 32,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let selected = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    ExecutionRequestV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        target: ExecutionTarget::local(),
        controller_target_id: None,
        workspace: WorkspaceRef {
            project_id,
            task_id,
            source_sha: "a".repeat(40),
        },
        mode,
        image_digest: selected.digest,
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        argv: vec!["git".into()],
        working_directory_relative: ".".into(),
        stdin: None,
        environment_allowlist: BTreeSet::new(),
        resource_budget: budget.clone(),
        deadline_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + crate::execution::contract::MAX_EXECUTION_DURATION_MS,
        allowed_service_ports: BTreeSet::new(),
        broker_capability_ids: BTreeSet::new(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(mode, budget, BTreeSet::new()).unwrap(),
    }
}

fn identity() -> SandboxIdentity {
    SandboxIdentity {
        owner_instance_id: uuid::Uuid::new_v4().to_string(),
        policy_version: "v1".into(),
        lease_expires_unix_ms: u64::MAX,
    }
}

fn selected_image() -> SelectedImage {
    ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap()
}

#[test]
fn container_spec_is_nonroot_isolated_and_resource_bounded() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let body = create_body(
        &request(ExecutionModeV1::Implementation),
        &workspace,
        &identity(),
        &selected_image(),
    )
    .unwrap();
    assert_eq!(body.user.as_deref(), Some("10001:10001"));
    let host = body.host_config.unwrap();
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.userns_mode, None);
    assert_eq!(host.cap_drop, Some(vec!["ALL".into()]));
    assert_eq!(host.memory, Some(1024 * 1024 * 1024));
    assert_eq!(host.memory_swap, host.memory);
    assert_eq!(host.pids_limit, Some(32));
    assert!(
        host.mounts
            .unwrap()
            .iter()
            .any(|mount| mount.target.as_deref() == Some("/workspace/.git")
                && mount.read_only == Some(true))
    );
}

#[test]
fn container_mounts_only_the_git_metadata_and_bounded_private_tmpfs() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let body = create_body(
        &request(ExecutionModeV1::Implementation),
        &workspace,
        &identity(),
        &selected_image(),
    )
    .unwrap();
    let host = body.host_config.unwrap();
    let mounts = host.mounts.unwrap();
    let binds: Vec<_> = mounts
        .iter()
        .filter(|mount| mount.typ == Some(MountType::BIND))
        .collect();

    assert_eq!(mounts.len(), 1);
    assert_eq!(binds.len(), 1);
    assert_eq!(
        binds[0].source.as_deref(),
        Some(workspace.git_metadata().to_str().unwrap())
    );
    assert_eq!(binds[0].target.as_deref(), Some("/workspace/.git"));
    assert_eq!(binds[0].read_only, Some(true));
    assert_eq!(
        host.tmpfs
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/home/executor", "/run", "/tmp", "/workspace"])
    );
}

#[test]
fn container_lease_label_uses_the_durable_expiry_value() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let request = request(ExecutionModeV1::Implementation);
    let lease_expires_at = request.deadline_unix_ms.saturating_add(60_000);
    let mut identity = identity();
    identity.lease_expires_unix_ms = lease_expires_at;
    let body = create_body(&request, &workspace, &identity, &selected_image()).unwrap();
    let expected = lease_expires_at.to_string();
    assert_eq!(
        body.labels.unwrap().get(LEASE_LABEL).map(String::as_str),
        Some(expected.as_str())
    );
}

#[test]
fn planning_workspace_is_staged_before_it_is_sealed_and_git_is_always_read_only() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let body = create_body(
        &request(ExecutionModeV1::PlanningReadOnly),
        &workspace,
        &identity(),
        &selected_image(),
    )
    .unwrap();
    let host = body.host_config.unwrap();
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("rw,"));
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("noexec,"));
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("mode=1777"));
    let mounts = host.mounts.unwrap();
    assert!(mounts.iter().any(|mount| {
        mount.target.as_deref() == Some("/workspace/.git") && mount.read_only == Some(true)
    }));
}

#[test]
fn writable_workspace_uses_bounded_tmpfs_and_keeps_git_read_only() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let body = create_body(
        &request(ExecutionModeV1::Implementation),
        &workspace,
        &identity(),
        &selected_image(),
    )
    .unwrap();
    let host = body.host_config.unwrap();
    let mounts = host.mounts.unwrap();
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("rw,"));
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("exec,"));
    assert!(!host.tmpfs.as_ref().unwrap()["/workspace"].contains("noexec"));
    assert!(host.tmpfs.as_ref().unwrap()["/workspace"].contains("mode=1777"));
    assert!(mounts.iter().any(|mount| {
        mount.target.as_deref() == Some("/workspace/.git") && mount.read_only == Some(true)
    }));
}

#[cfg(unix)]
#[test]
fn symlinked_git_metadata_is_refused() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.0.with_extension("git");
    fs::create_dir_all(&outside).unwrap();
    fs::remove_dir(fixture.0.join(".git")).unwrap();
    symlink(outside, fixture.0.join(".git")).unwrap();
    assert!(ValidatedWorkspace::inspect(&fixture.0).is_err());
}

#[test]
fn replacing_git_directory_at_same_path_invalidates_the_workspace() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let saved = fixture.0.join(".git-saved");
    fs::rename(fixture.0.join(".git"), &saved).unwrap();
    fs::create_dir(fixture.0.join(".git")).unwrap();

    assert!(workspace.revalidate().is_err());
}

#[test]
fn mutable_or_unapproved_image_references_are_refused() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let request = request(ExecutionModeV1::Implementation);
    let mut image = selected_image();
    image.reference = "debian:latest".into();
    assert!(create_body(&request, &workspace, &identity(), &image).is_err());
    image = selected_image();
    image.digest = format!("sha256:{}", "c".repeat(64));
    assert!(create_body(&request, &workspace, &identity(), &image).is_err());
}
