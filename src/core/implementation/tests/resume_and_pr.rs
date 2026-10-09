use super::*;

#[path = "resume_and_pr/branch_intent.rs"]
mod branch_intent;
#[path = "resume_and_pr/pull_request_state.rs"]
mod pull_request_state;
#[path = "resume_and_pr/task_clone_resume.rs"]
mod task_clone_resume;

#[test]
fn accepting_resume_moves_blocked_state_to_preparing_and_keeps_failure_history() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls).is_err());
    let mut blocked = load(&s.repo, &s.ticket).unwrap();
    blocked.status = ImplementationStatus::Blocked;
    blocked.detail = "NuGet audit feed was unreachable".into();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    save(&dir, &blocked).unwrap();

    let resumed = crate::core::implementation::mark_resume_started(&s.repo, &s.ticket)
        .unwrap()
        .unwrap();

    assert_eq!(resumed.status, ImplementationStatus::Preparing);
    assert!(resumed.detail.contains("Resume accepted"));
    assert!(resumed.detail.contains("NuGet audit feed was unreachable"));
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Preparing
    );
    let history = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-resume-context.txt")
        })
        .unwrap();
    assert_eq!(
        fs::read_to_string(history).unwrap(),
        "NuGet audit feed was unreachable"
    );

    let failed = crate::core::implementation::record_failed_attempt(
        &s.repo,
        &s.ticket,
        "Retry failed before implementation began: task lock is busy.",
    )
    .unwrap()
    .unwrap();
    assert_eq!(failed.status, ImplementationStatus::Blocked);
    assert_eq!(
        failed.detail,
        "Retry failed before implementation began: task lock is busy."
    );
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Blocked
    );
}

#[test]
fn active_resume_overrides_a_persisted_blocked_board_status() {
    let s = Sandbox::new();
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    state.status = ImplementationStatus::Blocked;

    assert_eq!(board_column(Some(&state), true), 1);
    assert_eq!(board_column(Some(&state), false), 3);
}

#[test]
fn feature_named_workspace_ticket_resumes_preserved_work() {
    let mut s = Sandbox::new();
    let ticket = ".koolade-packet/planning/tasks/feature/CHG-003-TASK-verify-workspace.md";
    fs::create_dir_all(s.repo.join(ticket).parent().unwrap()).unwrap();
    fs::rename(s.repo.join(&s.ticket), s.repo.join(ticket)).unwrap();
    s.ticket = ticket.into();
    s.git(&s.repo, &["add", "."]);
    s.git(&s.repo, &["commit", "-qm", "feature-named ticket"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let mut state = load(&s.repo, ticket).unwrap();
    state.status = ImplementationStatus::Blocked;
    save(&state_dir(&s.repo, ticket).unwrap(), &state).unwrap();
    let resumed = s.run("resume", calls.clone()).unwrap();
    assert_ne!(resumed.task_repository, state.task_repository);
    assert!(resumed.task_repositories.contains(&state.task_repository));
    assert!(resumed.branch.starts_with("koolade/integration/"));
    assert_eq!(resumed.base_commit, state.base_commit);
    assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn implementation_reads_only_canonical_board_task_paths() {
    let s = Sandbox::new();
    let root = ".koolade-packet/planning/tasks";
    fs::create_dir_all(s.repo.join(root).join("validation")).unwrap();
    for name in [
        "001-task.md",
        "CHG-003-TASK-verify.md",
        "F10-TASK-verify.md",
        "README.md",
        "specification.md",
        "001-task.txt",
        "invalid-TASK-verify.md",
    ] {
        let path = format!("{root}/validation/{name}");
        fs::write(s.repo.join(&path), "# Fixture task").unwrap();
        assert_eq!(
            read_ticket(&s.repo, &path).is_ok(),
            crate::artifacts::task_docs::is_task_story_filename(name),
            "{path}"
        );
    }
    fs::create_dir_all(s.repo.join("planning/tasks/validation")).unwrap();
    fs::write(
        s.repo.join("planning/tasks/validation/001-task.md"),
        "# Legacy fixture task",
    )
    .unwrap();
    assert!(read_ticket(&s.repo, "planning/tasks/validation/001-task.md").is_err());
    assert!(read_ticket(&s.repo, ".koolade-packet/planning/tasks/../001-task.md").is_err());
}

#[test]
fn new_task_includes_latest_remote_without_touching_checkout() {
    let s = Sandbox::new();
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let latest = s.advance_remote();
    fs::write(s.repo.join("draft.txt"), "keep my draft").unwrap();
    let result = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    assert_eq!(result.base_commit, latest);
    assert_eq!(
        fs::read_to_string(result.task_repository.join("upstream.txt")).unwrap(),
        "latest upstream\n"
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
    assert_eq!(
        fs::read_to_string(s.repo.join("draft.txt")).unwrap(),
        "keep my draft"
    );
    assert!(!s.repo.join("upstream.txt").exists());
}

#[test]
fn fetch_failure_stops_before_agent_runs() {
    let s = Sandbox::new();
    s.git(
        &s.repo,
        &["remote", "set-url", "origin", "/nonexistent-koolade-remote"],
    );
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let error = s.run("complete", calls.clone()).unwrap_err().to_string();
    assert!(error.contains("failed"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
    assert!(load(&s.repo, &s.ticket).is_none());
}

#[test]
fn local_commits_ahead_of_remote_are_preserved() {
    let s = Sandbox::new();
    let remote_base = s.git(
        &s.root.join("remote.git"),
        &["rev-parse", "refs/heads/main"],
    );
    fs::write(s.repo.join("local.txt"), "local change").unwrap();
    s.git(&s.repo, &["add", "."]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let local_source = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    assert_eq!(state.base_commit, remote_base);
    assert_eq!(state.source_commit.as_deref(), Some(local_source.as_str()));
    assert!(state.task_repository.join("local.txt").exists());
}

#[test]
fn blocked_or_failed_verification_never_creates_pr() {
    for mode in ["blocked", "fail"] {
        let s = Sandbox::new();
        let outcome = s.run(mode, Arc::new(AtomicUsize::new(0)));
        assert!(
            outcome.is_err(),
            "mode {mode} must fail: {:?}",
            outcome.ok()
        );
        assert!(!s.root.join("pr-created").exists());
        let Some(state) = load(&s.repo, &s.ticket) else {
            panic!(
                "mode {mode}: run failed ({outcome:?}) before persisting workflow state; \n\
                 a git/io-level infrastructure fault is suspected rather than the \n\
                 verification mode under test"
            );
        };
        assert!(state.task_repository.join("implemented.txt").exists());
    }
}
#[test]
fn external_decision_stops_after_one_report_and_preserves_work() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let result = s.run("external_blocked", calls.clone());
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(
        state
            .detail
            .contains("Adjudicator: approve the revised contract")
    );
    assert!(state.task_repository.join("implemented.txt").exists());
    assert!(!s.root.join("pr-created").exists());
}
#[test]
fn feature_id_collision_does_not_inject_another_features_specification() {
    assert!(!specification_matches_task(
        "# Task\n\nFeature: Switch Workspaces\n",
        "# CHG-003: Readable Chat Replies\n"
    ));
    assert!(specification_matches_task(
        "# Task\n\nFeature: Switch Workspaces\n",
        "# CHG-003: Switch Workspaces\n"
    ));
}
