use super::*;
use support::{Fixture, request, wait_for_stdout, workspace};
#[path = "daemon_restart/harness.rs"]
mod harness;
use harness::{IsolatedContainerCleanup, IsolatedRootlessDaemon};

const OPT_IN: &str = "KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION";

/// The isolated rootless daemon can answer its readiness ping before the full
/// API stack is servable under a heavily loaded CI runner. The first real
/// start therefore rides out the narrow engine-disconnect signature for a
/// bounded window; every other failure is fatal exactly as before, and the
/// invariants (dirty-clone preservation, no command replay, durable journal)
/// are unchanged.
async fn prepare_and_start_through_boot_window(
    state_home: &std::path::Path,
    backend: &crate::execution::docker::backend::DockerExecutionBackend,
    request: &crate::execution::contract::ExecutionRequestV1,
) -> (
    crate::execution::backend::WorkspaceLeaseV1,
    crate::execution::backend::ExecutionHandleV1,
) {
    const MAX_ATTEMPTS: u32 = 8;
    const PAUSE: std::time::Duration = std::time::Duration::from_secs(2);
    let boot_disconnect = |error: &crate::execution::errors::ExecutionError| {
        matches!(
            error.kind,
            crate::execution::errors::ExecutionErrorKind::ImageUnavailable
                | crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable
        ) && error.message.contains("Docker Engine disconnected")
    };
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        if attempt > 1 {
            // A failed attempt can leave the setup-task ledger mid-transition
            // (the image preparation request already recorded but never
            // completed). The ledger lives in the per-test scratch state
            // home, so retiring it here keeps every attempt honest at
            // version 1 instead of racing the version fence.
            let _ = std::fs::remove_file(state_home.join("machine-setup").join("tasks.json"));
        }
        let lease = match backend
            .prepare_workspace(&request.workspace, &request.target)
            .await
        {
            Ok(lease) => lease,
            Err(error) => {
                if !boot_disconnect(&error) || attempt >= MAX_ATTEMPTS {
                    panic!(
                        "prepare task workspace (attempt {attempt}): {error}{}",
                        boot_give_up_note(&error, &boot_disconnect, attempt, MAX_ATTEMPTS)
                    )
                }
                tokio::time::sleep(PAUSE).await;
                continue;
            }
        };
        match backend.start(&lease, request).await {
            Ok(handle) => return (lease, handle),
            Err(error) => {
                if !boot_disconnect(&error) || attempt >= MAX_ATTEMPTS {
                    panic!(
                        "start isolated container (attempt {attempt}): {error}{}",
                        boot_give_up_note(&error, &boot_disconnect, attempt, MAX_ATTEMPTS)
                    )
                }
                drop(lease);
                tokio::time::sleep(PAUSE).await;
            }
        }
    }
}

fn boot_give_up_note(
    error: &crate::execution::errors::ExecutionError,
    boot_disconnect: &dyn Fn(&crate::execution::errors::ExecutionError) -> bool,
    attempt: u32,
    max_attempts: u32,
) -> String {
    if boot_disconnect(error) {
        format!(
            " (transient engine disconnects persisted through all {attempt} of {max_attempts} boot-window attempts)"
        )
    } else {
        " (not the transient engine-disconnect signature)".into()
    }
}

/// Production boundary: the durable execution record and owned task clone are
/// reconciled across an actual Docker Engine process restart. A unit test or a
/// stopped-container simulation cannot prove the replacement worker reconnects
/// to a restarted daemon and refuses to replay a command that already began.
/// Requires Linux, `dockerd-rootless.sh`, RootlessKit, subordinate uid/gid maps,
/// and a pullable locked image. Setup cost is high: this starts a second isolated
/// rootless Engine with fresh storage and may pull the image. CI runs this test
/// in a dedicated focused invocation; no host/global Docker daemon is stopped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restarted_rootless_daemon_preserves_dirty_clone_without_command_replay() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping isolated daemon restart E2E; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 requires Linux rootless Docker support");
    }

    let fixture = Fixture::new();
    // Keep the shared machine-setup ledger (and everything else that honors
    // KOOLADE_HOME) inside this fixture for the lifetime of the test: the
    // boot-window retry retires the scratch ledger between attempts. Declared
    // before every other guard so it is restored last.
    let state_home = fixture.root.join("state-home");
    std::fs::create_dir_all(&state_home).expect("create per-test state home");
    let mut state_home_guard = crate::test_support::ProcessEnvironmentGuard::new();
    state_home_guard.set("KOOLADE_HOME", state_home.to_string_lossy().to_string());
    let mut daemon = IsolatedRootlessDaemon::start(&fixture.root).await;
    let endpoint = daemon.endpoint().clone();
    let docker = endpoint
        .connect()
        .expect("isolated daemon endpoint must be a trusted socket");
    let info = docker
        .info()
        .await
        .expect("isolated daemon must answer Docker info");
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "daemon recovery must use an actual rootless Docker Engine"
    );
    let mut docker_environment = crate::test_support::ProcessEnvironmentGuard::new();
    docker_environment.remove("DOCKER_CONTEXT");
    match &endpoint {
        crate::execution::docker::engine::DockerEndpoint::Unix { path, .. } => {
            docker_environment.set("DOCKER_HOST", format!("unix://{}", path.display()));
        }
        _ => panic!("isolated daemon recovery requires a Unix socket"),
    }
    let workspace = workspace();
    let (backend, task_clone) = backend_for_endpoint(
        &fixture,
        workspace.clone(),
        endpoint.clone(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let dirty_file = task_clone.join("unfinished-change.txt");
    fs::write(&dirty_file, "preserved uncommitted content\n").unwrap();
    let request = request(
        workspace.clone(),
        "printf 'container-only change\\n' > unfinished-change.txt; printf 'restart-ready\\n'; sleep 300; printf 'must-not-complete\\n'",
    );
    let _container_cleanup = IsolatedContainerCleanup::new(
        endpoint.clone(),
        format!("koolade-exec-{}", request.execution_id),
    );
    let (_lease, handle) =
        prepare_and_start_through_boot_window(&state_home, &backend, &request).await;

    let backend = Arc::new(backend);
    let running_backend = backend.clone();
    let running_request = request.clone();
    let running_handle = handle.clone();
    let running = tokio::spawn(async move {
        running_backend
            .exec(&running_handle, &running_request)
            .await
    });
    wait_for_stdout(&backend, &handle, "restart-ready").await;
    running.abort();
    let _ = running.await;
    let owner_id = backend.identity.owner_instance_id.clone();
    drop(backend);

    daemon.restart().await;
    let (restarted, _) = backend_for_endpoint(&fixture, workspace, endpoint, owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker must reconcile through the restarted daemon");

    assert_eq!(
        fs::read_to_string(&dirty_file).unwrap(),
        "preserved uncommitted content\n",
        "daemon restart must preserve pre-existing dirty task-clone data"
    );
    let record =
        crate::persistence::execution_records::load(&request.task_id, &request.execution_id)
            .unwrap()
            .expect("interrupted execution record remains durable");
    assert!(record.command_started);
    assert_eq!(
        record.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Interrupted
    );
    let duplicate_attempt = restarted.exec(&handle, &request).await.unwrap_err();
    assert_eq!(
        duplicate_attempt.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
        "a command that started before daemon restart must never be replayed"
    );
    let events = restarted
        .events(&handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("durable event journal remains readable");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                &event.event,
                ExecutionEventKindV1::Stdout { chunk } if chunk.contains("restart-ready")
            ))
            .count(),
        1,
        "restart recovery must not append output from a duplicate command"
    );
    assert!(events.iter().any(|event| matches!(
        event.event,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Interrupted
        }
    )));
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("interrupted result remains available");
    assert_eq!(result.exit_code, None);
    assert_eq!(
        result.failure.map(|failure| failure.kind),
        Some(crate::execution::errors::ExecutionErrorKind::WorkerDisconnected)
    );
}

fn backend_for_endpoint(
    fixture: &Fixture,
    workspace: crate::execution::contract::WorkspaceRef,
    endpoint: crate::execution::docker::engine::DockerEndpoint,
    owner_id: String,
) -> (
    crate::execution::docker::backend::DockerExecutionBackend,
    std::path::PathBuf,
) {
    let clone = fixture.task_clone(&workspace);
    let projects = fixture.projects_root();
    let resolver = Arc::new(
        crate::execution::docker::backend::OwnedTaskWorkspaceResolver::new(
            &projects, workspace, &clone,
        )
        .expect("synthetic task clone must be accepted"),
    );
    let backend = crate::execution::docker::backend::DockerExecutionBackend::connect(
        endpoint, resolver, owner_id,
    )
    .expect("connect to the isolated rootless Engine");
    (backend, clone)
}
