use super::*;

/// Runs the real implementation runner through cancellation, task-clone
/// resume, Docker verification, and its publication controller. Docker must be
/// the rootless Engine provisioned by the CI E2E job.
#[test]
fn cancelled_task_clone_resumes_through_docker_verification_and_waits_for_pr_authorization() {
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping task lifecycle Docker E2E; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1");
        return;
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("skipping task lifecycle Docker E2E; it requires Linux");
        return;
    }

    let mut s = Sandbox::new();
    // Production builds have no test executor. Removing this fixture hook
    // makes Runner::verify use ExecutionSession::connect_local, the same
    // Docker-backed verification path used by the application.
    s.use_production_verification();

    let calls = Arc::new(AtomicUsize::new(0));
    let user_status = s.git(&s.repo, &["status", "--porcelain"]);
    assert!(s.run("cancel", calls.clone()).is_err());

    let interrupted = load(&s.planning_store.root, &s.ticket).expect("cancelled state is durable");
    assert_eq!(interrupted.status, ImplementationStatus::Interrupted);
    assert_eq!(interrupted.task_repository_kind, TaskRepositoryKind::Clone);
    assert_eq!(
        fs::read_to_string(interrupted.task_repository.join("implemented.txt")).unwrap(),
        "partial\n"
    );
    assert!(!s.root.join("pr-created").exists());

    let resumed = match s.run("resume", calls.clone()) {
        Ok(resumed) => resumed,
        Err(error) if error.to_string().contains("Verification outcome unknown") => {
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            let blocked = load(&s.planning_store.root, &s.ticket)
                .expect("uncertain Docker verification state is durable");
            assert_eq!(blocked.status, ImplementationStatus::Blocked);
            assert!(blocked.detail.contains("no correction was dispatched"));
            assert_eq!(
                fs::read_to_string(blocked.task_repository.join("implemented.txt")).unwrap(),
                "implemented\n"
            );
            assert!(!s.root.join("pr-created").exists());
            assert_eq!(s.git(&s.repo, &["status", "--porcelain"]), user_status);
            let evidence_dir = state_dir(&s.planning_store.root, &s.ticket).unwrap();
            let evidence = fs::read_dir(evidence_dir)
                .unwrap()
                .flatten()
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with("-verification.json")
                })
                .expect("uncertain Docker verification evidence is retained");
            let evidence: serde_json::Value =
                serde_json::from_slice(&fs::read(evidence.path()).unwrap()).unwrap();
            assert!(
                evidence[0]["error"]
                    .as_str()
                    .unwrap()
                    .contains("outcome is unknown")
            );
            return;
        }
        Err(error) => panic!("Docker-backed resume failed: {error:#}"),
    };

    let actual_calls = calls.load(Ordering::SeqCst);
    let correction_summaries = if actual_calls == 2 {
        Vec::new()
    } else {
        let state_dir =
            crate::core::implementation::state_dir(&s.planning_store.root, &s.ticket).unwrap();
        std::fs::read_dir(state_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with("-correction.txt")
            })
            .map(|entry| {
                let summary = std::fs::read_to_string(entry.path())
                    .unwrap_or_default()
                    .lines()
                    .take(2)
                    .map(|line| {
                        if line.contains('/') || line.contains('\\') {
                            "<path-redacted>"
                        } else {
                            line
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                (entry.file_name(), summary)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        actual_calls, 2,
        "unexpected implementation retry; correction evidence: {correction_summaries:#?}"
    );
    assert_eq!(resumed.task_repository_kind, TaskRepositoryKind::Clone);
    assert_ne!(resumed.task_repository, interrupted.task_repository);
    assert!(
        resumed
            .task_repositories
            .contains(&interrupted.task_repository)
    );
    assert_eq!(resumed.status, ImplementationStatus::AwaitingApproval);
    assert!(resumed.pr_url.is_none());
    assert_eq!(
        fs::read_to_string(resumed.task_repository.join("implemented.txt")).unwrap(),
        "implemented\n"
    );
    assert!(!s.root.join("pr-created").exists());
    assert_eq!(s.git(&s.repo, &["status", "--porcelain"]), user_status);
    assert_eq!(
        fs::read_to_string(s.repo.join("implemented.txt")).unwrap_or_default(),
        "",
        "implementation output must remain in the task clone"
    );
}
