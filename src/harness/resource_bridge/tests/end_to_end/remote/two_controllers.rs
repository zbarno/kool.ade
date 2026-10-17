use super::*;
use crate::execution::{
    contract::{ExecutionTarget, ResourceBudgetV1},
    session::{AuthorizationGenerationVerifier, ExecutionSession, RemoteSessionOptions},
};
use std::{
    fmt::Debug,
    path::Path,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[test]
fn concurrent_ssh_controllers_share_worker_and_reject_revoked_result() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    // This guards an intermittent cross-controller regression; repeat independent
    // worker setups in one hosted run so a single lucky schedule does not hide it.
    for _scenario in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path();
        let task_home_path = root_path.join("task-home");
        fs::create_dir_all(&task_home_path).unwrap();
        let _home = support::task_home(&task_home_path);
        let repository = root_path.join("repository");
        fs::create_dir_all(&repository).unwrap();
        support::initialize_project(&repository);
        fs::write(repository.join("README.md"), "synthetic shared worker\n").unwrap();
        support::commit(&repository);
        let clone_a = support::create_task_clone(&repository, &task_home_path, "controller-a");
        let clone_b = support::create_task_clone(&repository, &task_home_path, "controller-b");
        let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
        let sshd = sshd::SshdFixture::start(root_path, &worker_binary)
            .expect("CI must provide loopback sshd for concurrent controller E2E");
        sshd.restore_known_host(&worker_id).unwrap();
        let worker = sshd.worker_config(&worker_id);
        crate::persistence::remote_workers::register(
            &worker_id,
            &worker.host_alias,
            &worker.ssh_principal,
            &sshd.host_fingerprint,
        )
        .unwrap();

        let generation_a = uuid::Uuid::new_v4().hyphenated().to_string();
        let (authorization_a, active_a) = authorization::RevocableAuthorization::new(generation_a);
        let session_a = Arc::new(connect_session(
            &clone_a,
            &worker,
            Arc::new(authorization_a),
        ));
        let run_a = session_a.clone();
        let mut controller_a = Some(std::thread::spawn(move || {
            run_a.run_tool_command(
            "printf 'controller-a-started\\n'; sleep 30; printf 'stale-controller-a\\n' > stale-result.txt",
            Duration::from_secs(120),
        )
        }));
        wait_for_remote_stdout(
            &session_a,
            "controller-a-started",
            &mut controller_a,
            &root_path.join("sshd.log"),
        );

        let generation_b = uuid::Uuid::new_v4().hyphenated().to_string();
        let session_b = connect_session(
            &clone_b,
            &worker,
            Arc::new(authorization::TestAuthorization(generation_b)),
        );
        let output_b = session_b
            .run_tool_command(
                "printf 'controller-b-result\\n' > controller-b-result.txt",
                Duration::from_secs(120),
            )
            .expect("second SSH controller should execute concurrently on the worker");
        assert_eq!(output_b.result.exit_code, Some(0));
        assert_eq!(
            fs::read(clone_b.join("controller-b-result.txt")).unwrap(),
            b"controller-b-result\n"
        );

        active_a.store(false, std::sync::atomic::Ordering::Release);
        let stale_result = controller_a
            .take()
            .expect("first controller should still be running")
            .join()
            .expect("first controller thread should not panic");
        assert!(matches!(
            stale_result,
            Err(ref error)
                if error.kind == crate::execution::errors::ExecutionErrorKind::PolicyDenied
        ));
        assert!(
            !clone_a.join("stale-result.txt").exists(),
            "a controller whose authorization was revoked must not apply the worker result"
        );
        drop(session_a);
        drop(sshd);
    }
}

pub(super) fn wait_for_remote_stdout<T: Debug>(
    session: &ExecutionSession,
    expected: &str,
    controller: &mut Option<JoinHandle<T>>,
    sshd_log: &Path,
) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(handle) = session.active_handle_for_test()
            && let Ok(events) = crate::persistence::execution_records::load_events(
                &handle.task_id,
                &handle.execution_id,
                crate::execution::contract::EventPageRequestV1::new(0, 256).unwrap(),
            )
            && events.iter().any(|event| {
                matches!(
                    &event.event,
                    crate::execution::contract::ExecutionEventKindV1::Stdout { chunk }
                        if chunk.contains(expected)
                )
            })
        {
            return;
        }

        if controller.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = controller
                .take()
                .expect("finished controller is present")
                .join();
            panic!(
                "remote command exited before emitting {expected:?}: {result:?}; sshd log: {}",
                sshd_log_tail(sshd_log)
            );
        }
        if Instant::now() >= deadline {
            panic!(
                "remote command did not emit {expected:?} before the deadline; sshd log: {}",
                sshd_log_tail(sshd_log)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn sshd_log_tail(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|contents| {
            contents
                .lines()
                .rev()
                .take(30)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|error| format!("unavailable: {error}"))
}

pub(super) fn connect_session(
    task_clone: &std::path::Path,
    worker: &crate::execution::remote::ssh::SshWorkerConfig,
    authorization_verifier: Arc<dyn AuthorizationGenerationVerifier>,
) -> ExecutionSession {
    connect_session_with_ids(
        task_clone,
        worker,
        authorization_verifier,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    )
}

pub(super) fn connect_session_with_ids(
    task_clone: &std::path::Path,
    worker: &crate::execution::remote::ssh::SshWorkerConfig,
    authorization_verifier: Arc<dyn AuthorizationGenerationVerifier>,
    project_id: &str,
    task_id: &str,
) -> ExecutionSession {
    let source_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(task_clone)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let workspace = WorkspaceRef {
        project_id: project_id.to_owned(),
        task_id: task_id.to_owned(),
        source_sha: String::from_utf8(source_sha.stdout)
            .unwrap()
            .trim()
            .to_owned(),
    };
    ExecutionSession::connect_remote_with_task_clone(
        workspace,
        RemoteSessionOptions {
            target: ExecutionTarget::remote(worker.stable_id.clone()),
            worker,
            mode: ExecutionModeV1::Implementation,
            profile: RuntimeProfile::General,
            resources: ResourceBudgetV1 {
                cpu_millis: 2_000,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                process_count: 128,
                temporary_disk_bytes: 1024 * 1024 * 1024,
                output_bytes: 1024 * 1024,
            },
            destination_sha: None,
            authorization_verifier: Some(authorization_verifier),
            task_clone: Some(task_clone),
        },
    )
    .expect("remote controller should establish an approved worker session")
}
