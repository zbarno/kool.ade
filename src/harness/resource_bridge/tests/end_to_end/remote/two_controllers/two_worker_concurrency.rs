use super::*;
use std::sync::{Arc, Barrier};

/// Production boundary: independent execution sessions route distinct tasks to two trusted SSH workers.
/// Unit tests cannot cover SSH identity, remote Docker staging, execution, and result application together.
/// Dependencies: Linux rootless Docker, loopback sshd, and the worker binary; high setup cost for two workers.
#[test]
fn separate_controllers_run_distinct_tasks_on_two_workers_without_cross_applying_results() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    let root = tempfile::tempdir().unwrap();
    let task_home = root.path().join("task-home");
    fs::create_dir_all(&task_home).unwrap();
    let _home = support::task_home(&task_home);
    let repository = root.path().join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(
        repository.join("README.md"),
        "synthetic two-worker fixture\n",
    )
    .unwrap();
    support::commit(&repository);
    let clone_a = support::create_task_clone(&repository, &task_home, "controller-a");
    let clone_b = support::create_task_clone(&repository, &task_home, "controller-b");

    let worker_a_root = root.path().join("worker-a");
    let worker_b_root = root.path().join("worker-b");
    fs::create_dir_all(&worker_a_root).unwrap();
    fs::create_dir_all(&worker_b_root).unwrap();
    let worker_a_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let worker_b_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert_ne!(worker_a_id, worker_b_id);
    let sshd_a = sshd::SshdFixture::start_for_worker(&worker_a_root, &worker_binary, &worker_a_id)
        .expect("CI must provide loopback sshd for worker A");
    let sshd_b = sshd::SshdFixture::start_for_worker(&worker_b_root, &worker_binary, &worker_b_id)
        .expect("CI must provide loopback sshd for worker B");
    sshd_a.restore_known_host(&worker_a_id).unwrap();
    sshd_b.restore_known_host(&worker_b_id).unwrap();
    let worker_a = sshd_a.worker_config(&worker_a_id);
    let worker_b = sshd_b.worker_config(&worker_b_id);
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
    let task_a_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_b_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert_ne!(task_a_id, task_b_id);
    let generation_a = uuid::Uuid::new_v4().hyphenated().to_string();
    let generation_b = uuid::Uuid::new_v4().hyphenated().to_string();
    let session_a = Arc::new(connect_session_with_ids(
        &clone_a,
        &repository,
        &worker_a,
        Arc::new(authorization::TestAuthorization(generation_a)),
        &project_id,
        &task_a_id,
    ));
    let session_b = Arc::new(connect_session_with_ids(
        &clone_b,
        &repository,
        &worker_b,
        Arc::new(authorization::TestAuthorization(generation_b)),
        &project_id,
        &task_b_id,
    ));

    let barrier = Arc::new(Barrier::new(3));
    let run_a = session_a.clone();
    let gate_a = barrier.clone();
    let mut controller_a = Some(std::thread::spawn(move || {
        gate_a.wait();
        run_a.run_tool_command(
            "sleep 30; printf 'worker-a-result\\n' > worker-a-result.txt",
            Duration::from_secs(120),
        )
    }));
    let run_b = session_b.clone();
    let gate_b = barrier.clone();
    let mut controller_b = Some(std::thread::spawn(move || {
        gate_b.wait();
        run_b.run_tool_command(
            "sleep 30; printf 'worker-b-result\\n' > worker-b-result.txt",
            Duration::from_secs(120),
        )
    }));
    let execution_started = std::time::Instant::now();
    barrier.wait();

    let output_a = controller_a
        .take()
        .unwrap()
        .join()
        .expect("controller A thread should not panic")
        .expect("worker A should return its own task result");
    let output_b = controller_b
        .take()
        .unwrap()
        .join()
        .expect("controller B thread should not panic")
        .expect("worker B should return its own task result");
    assert!(
        execution_started.elapsed() < Duration::from_secs(50),
        "two 30-second remote tasks should finish concurrently, not serially"
    );
    assert_eq!(output_a.result.exit_code, Some(0));
    assert_eq!(output_b.result.exit_code, Some(0));
    assert_ne!(output_a.result.execution_id, output_b.result.execution_id);
    assert_eq!(
        fs::read(clone_a.join("worker-a-result.txt")).unwrap(),
        b"worker-a-result\n"
    );
    assert!(!clone_a.join("worker-b-result.txt").exists());
    assert_eq!(
        fs::read(clone_b.join("worker-b-result.txt")).unwrap(),
        b"worker-b-result\n"
    );
    assert!(!clone_b.join("worker-a-result.txt").exists());

    drop(session_a);
    drop(session_b);
    drop(sshd_a);
    drop(sshd_b);
}
