use super::*;
use crate::{
    core::task_claim::{
        ClaimExecutionAttemptStatus as AttemptStatus, ClaimLease, ClaimLeaseHandle,
    },
    execution::{
        contract::{ResourceBudgetV1, TargetPreference, WorkspaceRef},
        remote::client::RemoteWorkerClient,
    },
    harness::{ExecutionMode, LiveProgress, PlanningRequest},
    persistence::execution_environment_settings::TaskCapabilityRequirements,
};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex, atomic::AtomicBool, mpsc},
};

mod authorization;
use authorization::ObservedAuthorization;

#[test]
fn ssh_worker_loss_fences_attempt_and_retries_on_a_different_trusted_worker() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    // Production boundary: the actual auto scheduler probes trusted workers, reserves capacity,
    // pins the selected target, confirms a Git-backed claim loss after the worker process dies,
    // excludes that worker, and retries over authenticated SSH with rootless Docker. A unit
    // backend cannot prove this path composes with worker enrollment, host-key verification,
    // Docker staging, result fencing, and cleanup of a killed worker's labeled container.
    // Dependencies: rootless Docker CLI/daemon, sshd, and KOOLADE_TEST_WORKER_BINARY.
    // Setup cost: two worker keys/configs, two handshakes per scheduler pass, a bare Git remote,
    // and a label-scoped Docker cleanup after worker loss.
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    let root = tempfile::tempdir().unwrap();
    let task_home_path = root.path().join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let repository = root.path().join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(repository.join("README.md"), "synthetic retry fixture\n").unwrap();
    support::commit(&repository);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "worker-retry");

    let worker_a_root = root.path().join("worker-a");
    let worker_b_root = root.path().join("worker-b");
    fs::create_dir_all(&worker_a_root).unwrap();
    fs::create_dir_all(&worker_b_root).unwrap();
    let worker_a_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let worker_b_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let sshd_a = sshd::SshdFixture::start_for_worker(&worker_a_root, &worker_binary, &worker_a_id)
        .expect("CI must provide loopback sshd for worker A retry E2E");
    let sshd_b = sshd::SshdFixture::start_for_worker(&worker_b_root, &worker_binary, &worker_b_id)
        .expect("CI must provide loopback sshd for worker B retry E2E");
    let worker_a = sshd_a.worker_config(&worker_a_id);
    let worker_b = sshd_b.worker_config(&worker_b_id);
    sshd_a.restore_known_host(&worker_a_id).unwrap();
    sshd_b.restore_known_host(&worker_b_id).unwrap();

    let dispatcher_directory = root.path().join("ssh-bin");
    let dispatcher = install_ssh_dispatcher(
        &dispatcher_directory,
        &[
            (&worker_a_id, &worker_a.ssh_binary),
            (&worker_b_id, &worker_b.ssh_binary),
        ],
    );
    let _home = support::task_home_with_path(&task_home_path, &dispatcher_directory);
    for (worker_id, worker, sshd) in [
        (&worker_a_id, &worker_a, &sshd_a),
        (&worker_b_id, &worker_b, &sshd_b),
    ] {
        crate::persistence::remote_workers::register(
            worker_id,
            &worker.host_alias,
            &worker.ssh_principal,
            &sshd.host_fingerprint,
        )
        .unwrap();
        sshd.add_controller_known_host(worker_id).unwrap();
        let probe = RemoteWorkerClient::connect_for_probe(worker)
            .expect("fixture worker must pass the real SSH capability probe");
        let report = probe.probe();
        let capabilities = report
            .capabilities
            .as_ref()
            .expect("fixture worker must report its runtime capabilities");
        assert_eq!(
            capabilities.app_version,
            env!("CARGO_PKG_VERSION"),
            "fixture worker must run the same application version"
        );
        assert_eq!(
            capabilities.policy_hash,
            crate::execution::policy::policy_hash(),
            "fixture worker must report the current execution policy"
        );
        let architecture = report
            .architecture
            .as_deref()
            .expect("fixture worker must report its architecture");
        let mut test_lock = crate::execution::docker::images::ImageLock::bundled()
            .expect("controller must have a valid bundled image lock");
        let general_image = test_lock
            .select(RuntimeProfile::General, architecture)
            .expect("controller must have a general image for the worker architecture")
            .clone();
        assert!(
            capabilities
                .trusted_image_digests
                .contains(&general_image.digest),
            "fixture worker must report the exact locally locked general image"
        );
        // Model a reviewed runtime promotion in the test lock, then pass the live worker report
        // through the same promotion-gated trust implementation used by production enrollment.
        test_lock
            .profiles
            .get_mut("general")
            .expect("bundled lock must include the general profile")
            .iter_mut()
            .find(|image| image.digest == general_image.digest)
            .expect("selected image must remain in the general profile")
            .runtime_enabled = true;
        crate::persistence::remote_workers::trust_reported_capabilities_with_test_lock(
            worker_id, report, &test_lock,
        )
        .expect("test enrollment must pass the production promotion-gated trust path");
    }
    assert!(dispatcher.is_file());

    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let project_uid = uuid::Uuid::parse_str(&project_id).unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = super::planning_repositories::git_output(&repository, &["rev-parse", "HEAD"]);
    let (_planning_remote, planning_a, _planning_b) =
        super::planning_repositories::create(root.path());
    let lease = ClaimLease::acquire(&planning_a, project_uid, &task_id, "web", &source_sha)
        .unwrap()
        .expect("the retry E2E must acquire a real Git-backed task claim");
    let claim = Arc::new(ClaimLeaseHandle::new(lease));
    let observed_statuses = Arc::new(Mutex::new(Vec::new()));
    let authorization = Arc::new(ObservedAuthorization {
        claim: claim.clone(),
        statuses: observed_statuses.clone(),
    });
    let workspace = WorkspaceRef {
        project_id,
        task_id: task_id.clone(),
        source_sha,
    };
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    let request = PlanningRequest {
        mode: ExecutionMode::Implementation,
        task_id: Some(task_id),
        execution_workspace: Some(workspace.clone()),
        destination_sha: None,
        authorization_verifier: Some(authorization),
        prompt_manifest: None,
        reasoning_level: "medium".into(),
        telemetry_phase: Some("implementation".into()),
        repo_root: task_clone.clone(),
        runtime_config_source: Some(repository.clone()),
        prompt_body: "synthetic retry fixture".into(),
        system_instructions: String::new(),
        timeout: Duration::from_secs(90),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        review_identity: None,
    };
    let resources = ResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 2 * 1024 * 1024 * 1024,
        process_count: 128,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let allowed_targets = vec![
        TargetPreference::Worker(worker_a_id.clone()),
        TargetPreference::Worker(worker_b_id.clone()),
    ];
    let session = Arc::new(
        crate::harness::execution_security::auto_remote_session_for_retry_test(
            &request,
            workspace,
            RuntimeProfile::General,
            resources,
            TaskCapabilityRequirements::default(),
            allowed_targets,
            Vec::new(),
        )
        .expect("production auto scheduler must select an explicitly trusted worker"),
    );
    let first_worker_id = session.target_id().to_owned();
    assert!(first_worker_id == worker_a_id || first_worker_id == worker_b_id);
    let (first_sshd_log, first_worker_home) = if first_worker_id == worker_a_id {
        (
            worker_a_root.join("sshd.log"),
            worker_a_root.join("worker-state"),
        )
    } else {
        (
            worker_b_root.join("sshd.log"),
            worker_b_root.join("worker-state"),
        )
    };

    let running_session = session.clone();
    let mut controller = Some(std::thread::spawn(move || {
        running_session.run_tool_command(
            "printf 'first-worker-started\\n'; sleep 3; printf 'alternate-worker-recovered\\n' > retry-result.txt",
            Duration::from_secs(90),
        )
    }));
    two_controllers::wait_for_remote_stdout(
        &session,
        "first-worker-started",
        &mut controller,
        &first_worker_home,
        &first_sshd_log,
    );
    eprintln!("worker-retry E2E: selected worker emitted command output");
    let first_sshd = if first_worker_id == worker_a_id {
        &sshd_a
    } else {
        &sshd_b
    };
    first_sshd
        .terminate_worker_process_for_test()
        .expect("the selected worker process should be terminated for the loss E2E");
    eprintln!("worker-retry E2E: selected worker process terminated");
    // Killing the worker process does not terminate the container it started. In
    // rootless Docker, docker exec can keep the SSH stream open until that container
    // exits, so remove the fixture-owned container before waiting for retry completion.
    first_sshd
        .cleanup_worker_containers_for_test(&first_worker_id)
        .expect("lost worker's containers must be removed and verified by owner label");
    eprintln!("worker-retry E2E: selected worker containers removed");
    eprintln!("worker-retry E2E: waiting for fenced retry result");
    let output = controller
        .take()
        .expect("controller should still be running")
        .join()
        .expect("controller should not panic while retrying on another worker")
        .expect("confirmed worker loss should retry on another trusted worker");
    eprintln!("worker-retry E2E: retry result received");
    assert_eq!(output.result.exit_code, Some(0));
    assert_eq!(
        fs::read(task_clone.join("retry-result.txt")).unwrap(),
        b"alternate-worker-recovered\n"
    );
    let retry_session = session
        .retry_destination()
        .unwrap()
        .expect("confirmed loss should install the scheduler-selected retry session");
    assert_ne!(first_worker_id, retry_session.target_id());
    assert!(
        retry_session.target_id() == worker_a_id || retry_session.target_id() == worker_b_id,
        "retry must remain within the explicitly trusted target pool"
    );
    assert!(
        observed_statuses
            .lock()
            .unwrap()
            .contains(&Some(AttemptStatus::ConfirmedLost)),
        "the production scheduler must open the alternate target only after durable loss fencing"
    );
    assert_eq!(
        claim.execution_attempt_for_test().unwrap().status,
        AttemptStatus::Finished
    );
    drop(session);
    drop(retry_session);
    drop(sshd_a);
    drop(sshd_b);
}

fn install_ssh_dispatcher(
    directory: &std::path::Path,
    workers: &[(&str, &std::path::Path)],
) -> std::path::PathBuf {
    fs::create_dir_all(directory).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (worker_id, ssh_wrapper) in workers {
        script.push_str(&format!(
            "  *HostKeyAlias={worker_id}*) exec {} \"$@\" ;;\n",
            shell_quote(&ssh_wrapper.to_string_lossy())
        ));
    }
    script
        .push_str("esac\necho 'no SSH fixture matches the pinned worker identity' >&2\nexit 127\n");
    let path = directory.join("ssh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
