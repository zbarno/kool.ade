use std::collections::BTreeSet;

use crate::execution::{
    contract::{
        ExecutionModeV1, ExecutionRequestV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    docker::images::{ImageLock, RuntimeProfile},
    policy::ExecutionPolicyV1,
};

use super::{native_docker_arch, now_unix_ms};

pub fn request(workspace: WorkspaceRef, script: &str) -> ExecutionRequestV1 {
    let budget = ResourceBudgetV1 {
        cpu_millis: 1000,
        memory_bytes: 512 * 1024 * 1024,
        process_count: 32,
        temporary_disk_bytes: 64 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let image = ImageLock::bundled()
        .expect("bundled image lock")
        .select(RuntimeProfile::General, native_docker_arch())
        .expect("general image for native architecture");
    ExecutionRequestV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        project_id: workspace.project_id.clone(),
        task_id: workspace.task_id.clone(),
        target: ExecutionTarget::local(),
        controller_target_id: None,
        private_files_attached: false,
        workspace,
        destination_sha: None,
        mode: ExecutionModeV1::Implementation,
        image_digest: image.digest,
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        argv: vec!["sh".into(), "-s".into()],
        working_directory_relative: ".".into(),
        stdin: Some(script.into()),
        environment_allowlist: BTreeSet::new(),
        resource_budget: budget.clone(),
        deadline_unix_ms: now_unix_ms() + 120_000,
        allowed_service_ports: BTreeSet::new(),
        broker_capability_ids: BTreeSet::new(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Implementation,
            budget,
            BTreeSet::new(),
        )
        .expect("implementation policy"),
    }
}

pub fn verification_request_with_artifacts(
    workspace: WorkspaceRef,
    script: &str,
    artifact_paths: Vec<String>,
) -> ExecutionRequestV1 {
    let mut request = request(workspace, script);
    request.mode = ExecutionModeV1::Verification;
    let image = ImageLock::bundled()
        .expect("bundled image lock")
        .select(RuntimeProfile::Python, native_docker_arch())
        .expect("Python image for native architecture");
    request.image_digest = image.digest;
    request.argv = vec!["python".into(), "-c".into(), script.into()];
    request.stdin = None;
    request.policy = ExecutionPolicyV1::for_approved_recipe(
        request.resource_budget.clone(),
        "evidence".into(),
        "user-approval:docker-recovery-test".into(),
        "python".into(),
        vec!["-c".into(), script.into()],
        ".".into(),
        BTreeSet::new(),
    )
    .expect("exact approved verification recipe")
    .with_artifact_paths(artifact_paths)
    .expect("artifact policy should accept bounded relative paths")
    .with_source_output_paths(vec![
        "evidence/command-count.txt".into(),
        "evidence/result.txt".into(),
    ])
    .expect("recovery fixture should approve only its synthetic outputs");
    request
}
