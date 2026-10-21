use super::*;
use crate::core::task_claim::{ClaimLease, ClaimLeaseHandle, ClaimRecord};

#[test]
fn git_claim_takeover_fences_the_stale_remote_worker_result() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    let root = std::env::temp_dir().join(format!(
        "koolade-remote-claim-takeover-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let task_home_path = root.join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let repository = root.join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(repository.join("README.md"), "claim fenced remote task\n").unwrap();
    support::commit(&repository);
    let clone_a = support::create_task_clone(&repository, &task_home_path, "claim-controller-a");
    let clone_b = support::create_task_clone(&repository, &task_home_path, "claim-controller-b");
    let source_sha = git_output(&clone_a, &["rev-parse", "HEAD"]);
    let project_uid = uuid::Uuid::new_v4();
    let project_id = project_uid.hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let (planning_remote, planning_a, planning_b) = super::planning_repositories::create(&root);
    let lease_a = ClaimLease::acquire(&planning_a, project_uid, &task_id, "web", &source_sha)
        .unwrap()
        .expect("the first controller must acquire the Git-backed claim");
    let old_record = lease_a.record().unwrap().clone();
    let old_session = old_record.session_id.clone();
    let verifier_a = Arc::new(ClaimLeaseHandle::new(lease_a));

    let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
    let sshd = sshd::SshdFixture::start(&root, &worker_binary)
        .expect("CI must provide loopback sshd for claim takeover E2E");
    sshd.restore_known_host(&worker_id).unwrap();
    let worker = sshd.worker_config(&worker_id);
    crate::persistence::remote_workers::register(
        &worker_id,
        &worker.host_alias,
        &worker.ssh_principal,
        &sshd.host_fingerprint,
    )
    .unwrap();

    let session_a = Arc::new(super::two_controllers::connect_session_with_ids(
        &clone_a,
        &repository,
        &worker,
        verifier_a,
        &project_id,
        &task_id,
    ));
    let run_a = session_a.clone();
    let mut stale_controller = Some(std::thread::spawn(move || {
        run_a.run_tool_command(
            "printf 'stale-worker-started\\n'; sleep 5; printf 'stale-result\\n' > stale-result.txt",
            Duration::from_secs(120),
        )
    }));
    super::two_controllers::wait_for_remote_stdout(
        &session_a,
        "stale-worker-started",
        &mut stale_controller,
        &task_home_path,
        &root.join("sshd.log"),
    );

    // Install a genuinely expired Git claim object at the shared claim ref,
    // then perform takeover through the production CAS-backed API.
    let reference = crate::core::task_claim::reference(project_uid, &task_id).unwrap();
    let mut expired = old_record;
    expired.issued_at_utc = 1;
    expired.expires_at_utc = 1;
    let tree = git_output(&planning_b, &["rev-parse", "HEAD^{tree}"]);
    let encoded_record = serde_json::to_string(&expired).unwrap();
    let expired_commit = git_output(
        &planning_b,
        &[
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "commit-tree",
            &tree,
            "-m",
            &encoded_record,
        ],
    );
    support::run_git(
        &planning_b,
        &[
            "push",
            "--quiet",
            "--force",
            "origin",
            &format!("{expired_commit}:{reference}"),
        ],
    );
    let lease_b = ClaimLease::take_over_stale(
        &planning_b,
        project_uid,
        &task_id,
        "web",
        &old_session,
        &source_sha,
    )
    .unwrap();
    let takeover_record = lease_b.record().unwrap().clone();
    assert_eq!(takeover_record.takeover_history.len(), 1);
    let verifier_b = Arc::new(ClaimLeaseHandle::new(lease_b));
    let session_b = super::two_controllers::connect_session_with_ids(
        &clone_b,
        &repository,
        &worker,
        verifier_b,
        &project_id,
        &task_id,
    );
    let output_b = session_b
        .run_tool_command(
            "printf 'current-owner-result\\n' > current-owner-result.txt",
            Duration::from_secs(120),
        )
        .expect("the new Git claimant should execute on the same worker");
    assert_eq!(output_b.result.exit_code, Some(0));
    assert_eq!(
        fs::read(clone_b.join("current-owner-result.txt")).unwrap(),
        b"current-owner-result\n"
    );

    let stale_result = stale_controller
        .take()
        .expect("stale controller should still be running")
        .join()
        .expect("stale controller thread should not panic");
    assert!(matches!(
        stale_result,
        Err(ref error)
            if error.kind == crate::execution::errors::ExecutionErrorKind::PolicyDenied
    ));
    assert!(
        !clone_a.join("stale-result.txt").exists(),
        "the old claim holder must not apply a result after takeover"
    );
    let remote_record: ClaimRecord = serde_json::from_str(&git_output(
        &planning_remote,
        &["show", "-s", "--format=%B", &reference],
    ))
    .unwrap();
    assert_eq!(remote_record.session_id, takeover_record.session_id);
    assert_eq!(remote_record.takeover_history[0].session_id, old_session);

    drop(session_a);
    let after_stale_drop: ClaimRecord = serde_json::from_str(&git_output(
        &planning_remote,
        &["show", "-s", "--format=%B", &reference],
    ))
    .unwrap();
    assert_eq!(
        after_stale_drop.session_id, takeover_record.session_id,
        "dropping the stale owner must not release the current Git claim"
    );
    drop(session_b);
    drop(sshd);
    fs::remove_dir_all(root).unwrap();
}

fn git_output(root: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
