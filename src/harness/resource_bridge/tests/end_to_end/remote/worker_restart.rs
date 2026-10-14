use super::*;
use std::sync::mpsc;

#[test]
fn ssh_worker_restart_reconciles_started_exec_and_rejects_partial_result() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    let root = std::env::temp_dir().join(format!(
        "koolade-remote-worker-restart-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let task_home_path = root.join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let repository = root.join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(repository.join("README.md"), "synthetic restart fixture\n").unwrap();
    support::commit(&repository);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "worker-restart");
    let worker_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let sshd = sshd::SshdFixture::start(&root, &worker_binary)
        .expect("CI must provide loopback sshd for worker restart E2E");
    sshd.restore_known_host(&worker_id).unwrap();
    let worker = sshd.worker_config(&worker_id);
    crate::persistence::remote_workers::register(
        &worker_id,
        &worker.host_alias,
        &worker.ssh_principal,
        &sshd.host_fingerprint,
    )
    .unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    let (authorization, active) =
        authorization::RevocableAuthorization::new(generation, started_tx);
    let session_a = Arc::new(two_controllers::connect_session(
        &task_clone,
        &worker,
        Arc::new(authorization),
    ));
    let running_session = session_a.clone();
    let controller = std::thread::spawn(move || {
        running_session.run_tool_command(
            "printf 'restart-started\\n'; sleep 30; printf 'partial-stale-result\\n' > partial-result.txt",
            Duration::from_secs(120),
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(90))
        .expect("worker must acknowledge execution before disconnect");
    let handle = session_a
        .active_handle_for_test()
        .expect("controller should retain the active execution handle");
    let event_deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let events = crate::persistence::execution_records::load_events(
            &handle.task_id,
            &handle.execution_id,
            crate::execution::contract::EventPageRequestV1::new(0, 256).unwrap(),
        )
        .expect("loopback worker event journal should be visible to the fixture");
        if events.iter().any(|event| {
            matches!(
                &event.event,
                crate::execution::contract::ExecutionEventKindV1::Stdout { chunk }
                    if chunk.contains("restart-started")
            )
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < event_deadline,
            "remote command did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    session_a.terminate_remote_connection_for_test();
    active.store(false, std::sync::atomic::Ordering::Release);
    let interrupted = controller
        .join()
        .expect("controller thread should finish after worker restart");
    assert!(matches!(
        interrupted,
        Err(ref error)
            if matches!(
                error.kind,
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
                    | crate::execution::errors::ExecutionErrorKind::PolicyDenied
            )
    ));
    assert!(!task_clone.join("partial-result.txt").exists());

    let generation_b = uuid::Uuid::new_v4().hyphenated().to_string();
    let session_b = two_controllers::connect_session(
        &task_clone,
        &worker,
        Arc::new(authorization::TestAuthorization(generation_b)),
    );
    let output = session_b
        .run_tool_command(
            "printf 'worker-recovered\\n' > recovered-result.txt",
            Duration::from_secs(120),
        )
        .expect("a fresh SSH controller should use the recovered worker");
    assert_eq!(output.result.exit_code, Some(0));
    assert_eq!(
        fs::read(task_clone.join("recovered-result.txt")).unwrap(),
        b"worker-recovered\n"
    );
    drop(session_a);
    drop(sshd);
    fs::remove_dir_all(root).unwrap();
}
