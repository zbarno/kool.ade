use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::execution::{
    backend::{ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{
        ExecutionModeV1, ExecutionRequestV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    docker::{
        backend::{DockerExecutionBackend, OwnedTaskWorkspaceResolver},
        engine::{DockerEndpoint, HostPlatform},
        images::{ImageLock, RuntimeProfile},
    },
    policy::ExecutionPolicyV1,
};

#[path = "support/stdout.rs"]
mod stdout;
pub(super) use stdout::{wait_for_stdout, wait_for_stdout_with_task};

pub(super) const OPT_IN: &str = "KOOLADE_DOCKER_BACKEND_INTEGRATION";

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
    _home: crate::test_support::KooladeHomeGuard,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-docker-backend-it-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let home = std::env::temp_dir().join(format!(
            "koolade-docker-backend-home-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&home).unwrap();
        let git_init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&root)
            .status()
            .expect("git must be available for the Docker backend fixture");
        assert!(
            git_init.success(),
            "initialize the Docker backend fixture repository"
        );
        let home_guard = crate::test_support::KooladeHomeGuard::set(&home);
        Self {
            root,
            home,
            _home: home_guard,
        }
    }

    pub(super) fn task_clone(&self, workspace: &WorkspaceRef) -> PathBuf {
        let project = format!("{}-integration", workspace.project_id);
        let task = format!("task-{}", workspace.task_id);
        let path = self
            .home
            .join("projects")
            .join(project)
            .join("task-repositories")
            .join("integration")
            .join(task);
        fs::create_dir_all(path.join(".git")).unwrap();
        // Approved verification recipes are scoped to this directory. It must
        // exist in the authoritative host clone before Docker stages it in.
        fs::create_dir_all(path.join("evidence")).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.home);
    }
}

pub(super) struct ContainerCleanup(String);

impl Drop for ContainerCleanup {
    fn drop(&mut self) {
        // This exact UUID-derived name is reserved by the test. The fallback also
        // cleans up if an assertion aborts before the backend release call.
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.0])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

pub(super) fn workspace() -> WorkspaceRef {
    WorkspaceRef {
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        source_sha: "a".repeat(40),
    }
}

pub(super) fn request(workspace: WorkspaceRef, script: &str) -> ExecutionRequestV1 {
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

pub(super) fn verification_request_with_artifacts(
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
    .expect("artifact policy should accept bounded relative paths");
    request
}

pub(super) fn backend(
    fixture: &Fixture,
    workspace: WorkspaceRef,
) -> (DockerExecutionBackend, PathBuf) {
    backend_with_owner(
        fixture,
        workspace,
        uuid::Uuid::new_v4().hyphenated().to_string(),
    )
}

pub(super) fn backend_with_owner(
    fixture: &Fixture,
    workspace: WorkspaceRef,
    owner_id: String,
) -> (DockerExecutionBackend, PathBuf) {
    let clone = fixture.task_clone(&workspace);
    let projects = fixture.home.join("projects");
    let resolver = Arc::new(
        OwnedTaskWorkspaceResolver::new(&projects, workspace, &clone)
            .expect("synthetic task clone must be accepted"),
    );
    let endpoint = DockerEndpoint::from_process_environment(HostPlatform::Linux)
        .expect("opted-in test requires a supported local Docker Engine");
    let backend = DockerExecutionBackend::connect(endpoint, resolver, owner_id)
        .expect("connect to local Docker Engine");
    (backend, clone)
}

pub(super) fn container_cleanup(request: &ExecutionRequestV1) -> ContainerCleanup {
    ContainerCleanup(format!("koolade-exec-{}", request.execution_id))
}

pub(super) async fn prepare_and_start(
    backend: &DockerExecutionBackend,
    request: &ExecutionRequestV1,
) -> (WorkspaceLeaseV1, ExecutionHandleV1) {
    let lease = backend
        .prepare_workspace(&request.workspace, &request.target)
        .await
        .expect("prepare task workspace");
    let handle = backend
        .start(&lease, request)
        .await
        .expect("start isolated container");
    (lease, handle)
}

pub(super) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn container_id(backend: &DockerExecutionBackend, handle: &ExecutionHandleV1) -> String {
    backend
        .executions
        .lock()
        .unwrap()
        .get(&handle.execution_id)
        .unwrap()
        .lock()
        .unwrap()
        .container_id
        .clone()
}

pub(super) fn native_docker_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => panic!("unsupported Docker integration-test architecture"),
    }
}
