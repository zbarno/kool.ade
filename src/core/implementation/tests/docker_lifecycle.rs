use super::*;

/// Runs the real implementation runner through cancellation, task-clone
/// resume, Docker verification, and its publication controller. Docker must be
/// the rootless Engine provisioned by the CI E2E job.
#[test]
fn cancelled_task_clone_resumes_through_docker_verification_and_pr_preparation() {
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

    let interrupted = load(&s.repo, &s.ticket).expect("cancelled state is durable");
    assert_eq!(interrupted.status, ImplementationStatus::Interrupted);
    assert_eq!(interrupted.task_repository_kind, TaskRepositoryKind::Clone);
    assert_eq!(
        fs::read_to_string(interrupted.task_repository.join("implemented.txt")).unwrap(),
        "partial\n"
    );
    assert!(!s.root.join("pr-created").exists());

    let resumed = s
        .run("resume", calls.clone())
        .unwrap_or_else(|error| panic!("Docker-backed resume failed: {error:#}"));

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(resumed.task_repository_kind, TaskRepositoryKind::Clone);
    assert_ne!(resumed.task_repository, interrupted.task_repository);
    assert!(
        resumed
            .task_repositories
            .contains(&interrupted.task_repository)
    );
    assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        resumed.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        fs::read_to_string(resumed.task_repository.join("implemented.txt")).unwrap(),
        "implemented\n"
    );
    assert!(s.root.join("pr-created").exists());
    assert_eq!(s.git(&s.repo, &["status", "--porcelain"]), user_status);
    assert_eq!(
        fs::read_to_string(s.repo.join("implemented.txt")).unwrap_or_default(),
        "",
        "implementation output must remain in the task clone"
    );
}
