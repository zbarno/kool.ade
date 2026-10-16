use super::support::{Fixture, OPT_IN, native_docker_arch, request, workspace};
use crate::execution::{
    contract::{DependencyCacheEcosystemV1, ExecutionModeV1, ExecutionTarget, WorkspaceRef},
    docker::{
        backend::{DockerExecutionBackend, TaskWorkspaceResolver},
        engine::{DockerEndpoint, HostPlatform},
        images::{ImageLock, RuntimeProfile},
    },
    errors::ExecutionError,
    session::ExecutionSession,
};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};

struct TwoTaskResolver {
    clones: HashMap<WorkspaceRef, PathBuf>,
    staging: PathBuf,
}

impl TaskWorkspaceResolver for TwoTaskResolver {
    fn resolve_owned_clone(&self, workspace: &WorkspaceRef) -> Result<PathBuf, ExecutionError> {
        self.clones.get(workspace).cloned().ok_or_else(|| {
            ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "test resolver rejected another task workspace",
                false,
            )
        })
    }

    fn staging_root(&self) -> PathBuf {
        self.staging.clone()
    }
}

/// Production boundary: task-scoped cache snapshot registration and Docker
/// container startup. The invariant is that concurrent task containers receive
/// only the cache ref explicitly attached to their own session, with no worker
/// egress. A unit test cannot prove the actual Docker API installs each archive
/// into only its selected container. Requires Linux Docker and the locked base
/// image, with no network inside either task; moderate setup cost.
#[test]
fn concurrent_tasks_receive_only_their_own_dependency_cache_snapshot() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker cache isolation integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let task_a = workspace();
    let task_b = workspace();
    let clone_a = fixture.task_clone(&task_a);
    let clone_b = fixture.task_clone(&task_b);
    let resolver = Arc::new(TwoTaskResolver {
        clones: HashMap::from([(task_a.clone(), clone_a), (task_b.clone(), clone_b)]),
        staging: fixture.root.join("staging"),
    });
    fs::create_dir_all(&resolver.staging).unwrap();
    let endpoint = DockerEndpoint::from_process_environment(HostPlatform::Linux)
        .expect("opted-in test requires a supported local Docker Engine");
    let backend = Arc::new(
        DockerExecutionBackend::connect(
            endpoint,
            resolver,
            uuid::Uuid::new_v4().hyphenated().to_string(),
        )
        .expect("connect to local Docker Engine"),
    );
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, native_docker_arch())
        .unwrap();
    let resources = request(task_a.clone(), "").resource_budget;
    let session_a = Arc::new(
        ExecutionSession::new(
            backend.clone(),
            task_a.clone(),
            ExecutionTarget::local(),
            ExecutionModeV1::Implementation,
            image.digest.clone(),
            resources.clone(),
        )
        .unwrap(),
    );
    let session_b = Arc::new(
        ExecutionSession::new(
            backend,
            task_b.clone(),
            ExecutionTarget::local(),
            ExecutionModeV1::Implementation,
            image.digest,
            resources,
        )
        .unwrap(),
    );
    let cache_a = snapshot(&fixture.root, &task_a, session_a.as_ref(), "task-a-only");
    let cache_b = snapshot(&fixture.root, &task_b, session_b.as_ref(), "task-b-only");
    assert!(session_a.stage_dependency_cache(cache_b.clone()).is_err());
    assert!(session_b.stage_dependency_cache(cache_a.clone()).is_err());
    session_a.stage_dependency_cache(cache_a).unwrap();
    session_b.stage_dependency_cache(cache_b).unwrap();

    let run_a = || {
        session_a.run_tool_command(
            "set -eu; test -f /home/executor/.npm/task-a-only; test ! -e /home/executor/.npm/task-b-only; test \"$(wc -l < /proc/net/route)\" -eq 1; echo task-a-isolated",
            Duration::from_secs(90),
        )
    };
    let run_b = || {
        session_b.run_tool_command(
            "set -eu; test -f /home/executor/.npm/task-b-only; test ! -e /home/executor/.npm/task-a-only; test \"$(wc -l < /proc/net/route)\" -eq 1; echo task-b-isolated",
            Duration::from_secs(90),
        )
    };
    let (result_a, result_b) = std::thread::scope(|scope| {
        let a = scope.spawn(run_a);
        let b = scope.spawn(run_b);
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(result_a.unwrap().result.exit_code, Some(0));
    assert_eq!(result_b.unwrap().result.exit_code, Some(0));
}

fn snapshot(
    root: &std::path::Path,
    workspace: &WorkspaceRef,
    session: &ExecutionSession,
    marker: &str,
) -> crate::harness::resource_bridge::CacheSnapshot {
    let cache_root = root.join(marker);
    fs::create_dir_all(&cache_root).unwrap();
    fs::write(cache_root.join(marker), marker).unwrap();
    crate::harness::resource_bridge::CacheSnapshot::capture(
        &cache_root,
        &workspace.task_id,
        DependencyCacheEcosystemV1::Npm,
        &workspace.project_id,
        &format!("sha256:{}", "a".repeat(64)),
        &session.cache_target_arch().unwrap(),
        session.target_id(),
    )
    .unwrap()
}
