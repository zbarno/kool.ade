use super::*;
use crate::core::task_claim::{
    ClaimExecutionAttemptStatus as AttemptStatus, ClaimLease, ClaimLeaseHandle,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn ssh_worker_loss_fences_attempt_and_retries_on_a_different_trusted_worker() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    // Production boundary: real SSH worker sessions, durable Docker recovery,
    // production Git-backed task-claim transitions, and alternate-host retry selection. A
    // unit backend cannot prove SSH loss is reconciled before another host starts.
    // Dependencies: rootless Docker, sshd, and KOOLADE_TEST_WORKER_BINARY.
    // Setup cost: two temporary worker keys/configs, two handshakes, and a bare Git remote.
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    let root = tempfile::tempdir().unwrap();
    let task_home_path = root.path().join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let repository = root.path().join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(repository.join("README.md"), "synthetic retry fixture\n").unwrap();
    support::commit(&repository);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "worker-retry");

    let worker_a_root = root.path().join("worker-a");
    let worker_b_root = root.path().join("worker-b");
    let worker_a_home = worker_a_root.join("koolade-home");
    let worker_b_home = worker_b_root.join("koolade-home");
    fs::create_dir_all(&worker_a_home).unwrap();
    fs::create_dir_all(&worker_b_home).unwrap();
    let sshd_a = sshd::SshdFixture::start(&worker_a_root, &worker_binary)
        .expect("CI must provide loopback sshd for worker A retry E2E");
    let sshd_b = sshd::SshdFixture::start(&worker_b_root, &worker_binary)
        .expect("CI must provide loopback sshd for worker B retry E2E");
    let worker_a_id =
        crate::persistence::execution_records::owner_instance_id_at(&worker_a_home).unwrap();
    let worker_b_id =
        crate::persistence::execution_records::owner_instance_id_at(&worker_b_home).unwrap();
    sshd_a.restore_known_host(&worker_a_id).unwrap();
    sshd_b.restore_known_host(&worker_b_id).unwrap();
    let worker_a = sshd_a
        .worker_config_with_home(&worker_a_id, &worker_a_home)
        .unwrap();
    let worker_b = sshd_b
        .worker_config_with_home(&worker_b_id, &worker_b_home)
        .unwrap();
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
    }

    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let project_uid = uuid::Uuid::parse_str(&project_id).unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = super::planning_repositories::git_output(&repository, &["rev-parse", "HEAD"]);
    let (_planning_remote, planning_a, _planning_b) =
        super::planning_repositories::create(root.path());
    let lease = ClaimLease::acquire(&planning_a, project_uid, &task_id, "web", &source_sha)
        .unwrap()
        .expect("the retry E2E must acquire a real Git-backed task claim");
    let authorization = Arc::new(ClaimLeaseHandle::new(lease));
    let retry_observed_statuses = Arc::new(Mutex::new(Vec::new()));
    let session_a = Arc::new(two_controllers::connect_session_with_ids(
        &task_clone,
        &repository,
        &worker_a,
        authorization.clone(),
        &project_id,
        &task_id,
    ));
    let retry_clone = task_clone.clone();
    let retry_repository = repository.clone();
    let retry_worker = worker_b.clone();
    let retry_authorization = authorization.clone();
    let observed_statuses = retry_observed_statuses.clone();
    session_a
        .set_remote_retry_factory(Arc::new(move || {
            observed_statuses.lock().unwrap().push(
                retry_authorization
                    .execution_attempt_for_test()
                    .unwrap()
                    .status,
            );
            Ok(Arc::new(two_controllers::connect_session_with_ids(
                &retry_clone,
                &retry_repository,
                &retry_worker,
                retry_authorization.clone(),
                &project_id,
                &task_id,
            )))
        }))
        .unwrap();

    let running_session = session_a.clone();
    let mut controller = Some(std::thread::spawn(move || {
        running_session.run_tool_command(
            "printf 'worker-a-started\\n'; sleep 3; printf 'worker-b-recovered\\n' > retry-result.txt",
            Duration::from_secs(90),
        )
    }));
    two_controllers::wait_for_remote_stdout(
        &session_a,
        "worker-a-started",
        &mut controller,
        &worker_a_root.join("sshd.log"),
    );
    session_a.terminate_remote_connection_for_test();
    let output = controller
        .take()
        .expect("controller should still be running")
        .join()
        .expect("controller should not panic while retrying on worker B")
        .expect("confirmed worker loss should retry on the alternate trusted worker");

    assert_eq!(output.result.exit_code, Some(0));
    assert_eq!(
        fs::read(task_clone.join("retry-result.txt")).unwrap(),
        b"worker-b-recovered\n"
    );
    assert_eq!(
        *retry_observed_statuses.lock().unwrap(),
        vec![AttemptStatus::ConfirmedLost],
        "the alternate worker must start only after the Git claim records confirmed loss"
    );
    assert_eq!(
        authorization.execution_attempt_for_test().unwrap().status,
        AttemptStatus::Finished
    );
    assert_ne!(worker_a_id, worker_b_id);
    drop(session_a);
    drop(sshd_a);
    drop(sshd_b);
}
