use super::*;

#[test]
fn divergent_histories_are_combined_before_the_task_agent_runs() {
    let s = Sandbox::new();
    let remote = s.advance_remote();
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert!(
        s.git(
            &result.task_repository,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
    assert_eq!(
        fs::read_to_string(result.task_repository.join("local.txt")).unwrap(),
        "local change\n"
    );
    assert!(result.task_repository.join("implemented.txt").exists());
    assert_eq!(
        fs::read_to_string(result.task_repository.join("upstream.txt")).unwrap(),
        "latest upstream\n"
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
}

#[test]
fn merge_conflicts_are_agent_resolved_and_verified_in_isolation() {
    let s = conflict_sandbox();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let remote = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fs::read_to_string(result.task_repository.join("shared.txt")).unwrap(),
        "both edits preserved\n"
    );
    assert!(
        s.git(
            &result.task_repository,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
}

#[test]
fn ambiguous_conflicts_remain_blocked_without_touching_either_branch() {
    let s = conflict_sandbox();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: true,
        expect_snapshot: false,
        wrap_report: false,
    };

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("choose how shared.txt should behave"));
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(state.detail.contains("Keep local behavior"));
    assert!(!state.task_repository.join("implemented.txt").exists());
    assert!(
        !s.git(
            &state.task_repository,
            &["diff", "--name-only", "--diff-filter=U"]
        )
        .is_empty()
    );
}
