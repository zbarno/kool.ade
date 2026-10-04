use super::*;

#[test]
fn isolated_implementation_verifies_pushes_and_reuses_pr() {
    let s = Sandbox::new();
    fs::write(s.repo.join("unrelated.txt"), "main checkout draft").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let result = s.run("complete", calls.clone()).unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert!(result.pr_url.is_some());
    assert!(
        !crate::artifacts::layout::ArtifactLayout::new(&s.repo)
            .decisions_root()
            .exists(),
        "implementing a task alone must not create an ADR"
    );
    assert!(
        state_dir(&s.repo, &s.ticket)
            .unwrap()
            .join("verified-report.json")
            .exists(),
        "verification evidence stays in the implementation record"
    );
    assert!(!s.repo.join("implemented.txt").exists());
    assert!(!result.worktree.join("unrelated.txt").exists());
    assert_eq!(
        fs::read_to_string(s.repo.join("unrelated.txt")).unwrap(),
        "main checkout draft"
    );
    // Simulate a crash after GitHub created the PR but before its URL was saved.
    let mut recovery = result.clone();
    recovery.pr_url = None;
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &recovery).unwrap();
    let again = s.run("complete", calls.clone()).unwrap();
    assert_eq!(again.pr_url, result.pr_url);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(s.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn disabled_auto_publish_keeps_verified_work_local_until_explicit_pr_action() {
    let s = Sandbox::new();
    let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let calls = Arc::new(AtomicUsize::new(0));

    let held = s
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::AutoPublish,
            Some(Arc::new(AtomicBool::new(false))),
            false,
        )
        .unwrap();
    assert_eq!(held.status, ImplementationStatus::ReadyToPublish);
    assert!(held.pr_url.is_none() && held.merged_commit.is_none());
    assert!(held.worktree.exists());
    assert!(
        state_dir(&s.repo, &s.ticket)
            .unwrap()
            .join("verified-report.json")
            .exists()
    );
    assert!(!s.root.join("pr-created").exists());
    assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);

    let pr = s
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap();
    assert_eq!(pr.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        pr.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "verified implementation is reused"
    );
    assert!(s.root.join("pr-created").exists());
}

#[test]
fn required_independent_checks_fail_closed_for_unsupported_git_hosting() {
    let s = Sandbox::new();
    let unsupported_remote = "https://example.test/team/repo.git";
    s.git(
        &s.repo,
        &["config", "remote.origin.url", unsupported_remote],
    );
    s.git(
        &s.repo,
        &[
            "config",
            &format!("url.{}.insteadOf", s.root.join("remote.git").display()),
            unsupported_remote,
        ],
    );
    let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let error = s
        .run_with_publication_policy("complete", calls, PublicationMode::AutoPublish, None, true)
        .unwrap_err();
    assert!(error.to_string().contains("no supported CI provider"));
    let classified = Failure::from_error(&error);
    assert_eq!(classified.kind, FailureKind::ExternalPrerequisite);
    assert_eq!(classified.recovery, RecoveryDisposition::UserAction);
    assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
    let state_dir = state_dir(&s.repo, &s.ticket).unwrap();
    let state = read_state_file(&state_dir.join("state.json")).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    let check = state.independent_check.unwrap();
    assert_eq!(check.status, IndependentCheckStatus::Unavailable);
    assert!(check.detail.unwrap().contains("does not support"));
    assert!(state_dir.join("verified-report.json").exists());
}

#[test]
fn independent_check_gate_pushes_only_the_candidate_and_records_the_exact_result() {
    for passes in [true, false] {
        let s = Sandbox::new();
        let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut state = s.run("complete", calls).unwrap();
        assert!(state.worktree.exists());
        let commit = state.verified_head.clone().unwrap();
        let candidate_ref = checks::candidate_ref("fixture", &commit);
        let worktree = state.worktree.clone();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let (progress, _updates) = mpsc::channel();
        let runner = Runner {
            gh: s.gh.to_string_lossy().into_owned(),
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        let outcome = checks_gate::wait_with_provider(
            checks_gate::CheckRequest {
                provider: &ImmediateCheck(passes),
                repository: "github.com/fixture/repo",
                candidate_ref: &candidate_ref,
                worktree: &worktree,
                commit: &commit,
            },
            &dir,
            &mut state,
            &runner,
            None,
        );
        if passes {
            outcome.unwrap();
            assert_eq!(state.status, ImplementationStatus::Publishing);
            assert_eq!(
                state.independent_check.as_ref().unwrap().status,
                IndependentCheckStatus::Passed
            );
        } else {
            let error = outcome.unwrap_err();
            assert!(error.to_string().contains("Project checks failed"));
            let classified = Failure::from_error(&error);
            assert_eq!(classified.kind, FailureKind::Verification);
            assert_eq!(classified.recovery, RecoveryDisposition::ExplicitResume);
            assert_eq!(
                state.independent_check.as_ref().unwrap().status,
                IndependentCheckStatus::Failed
            );
        }
        assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
        assert!(
            s.git(&s.repo, &["ls-remote", "origin", &candidate_ref])
                .contains(&commit),
            "the exact integration commit should be on the temporary checks ref"
        );
    }
}

#[test]
fn disabling_auto_publish_during_successful_checks_keeps_default_branch_unchanged() {
    let s = Sandbox::new();
    let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut state = s.run("complete", calls).unwrap();
    let commit = state.verified_head.clone().unwrap();
    let candidate_ref = checks::candidate_ref("fixture", &commit);
    let worktree = state.worktree.clone();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: s.gh.to_string_lossy().into_owned(),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let gate = Arc::new(AtomicBool::new(true));
    checks_gate::wait_with_provider(
        checks_gate::CheckRequest {
            provider: &DisablingCheck(gate.clone()),
            repository: "github.com/fixture/repo",
            candidate_ref: &candidate_ref,
            worktree: &worktree,
            commit: &commit,
        },
        &dir,
        &mut state,
        &runner,
        Some(&gate),
    )
    .unwrap();

    assert!(!gate.load(Ordering::SeqCst));
    assert_eq!(state.status, ImplementationStatus::ReadyToPublish);
    assert_eq!(
        state.independent_check.as_ref().unwrap().status,
        IndependentCheckStatus::Passed
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
    assert!(
        s.git(&s.repo, &["ls-remote", "origin", &candidate_ref])
            .contains(&commit)
    );
}
#[test]
fn explicit_evidence_only_task_completes_without_commit_or_pr() {
    let s = Sandbox::new();
    s.make_evidence_only();
    let base = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let calls = Arc::new(AtomicUsize::new(0));

    let result = s.run("evidence_only", calls.clone()).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.status, ImplementationStatus::Completed);
    assert_eq!(result.verified_head.as_deref(), Some(base.as_str()));
    assert_eq!(result.merged_commit.as_deref(), Some(base.as_str()));
    assert!(result.pr_url.is_none());
    assert!(!s.root.join("pr-created").exists());
    assert!(
        result.cleanup.completed_at.is_some(),
        "{:?}",
        result.cleanup
    );
    assert!(!result.worktree.exists());
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), base);
    assert!(
        state_dir(&s.repo, &s.ticket)
            .unwrap()
            .join("verified-report.json")
            .exists()
    );
}
#[test]
fn verification_contract_completes_without_product_changes() {
    let s = Sandbox::new();
    fs::write(s.repo.join(&s.ticket), "# Verify regression\n\nThis ticket is pure verification, commits no bytes.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n\nThis ticket itself changed no repository file.\n").unwrap();
    let result = s
        .run("evidence_only", Arc::new(AtomicUsize::new(0)))
        .unwrap();
    assert_eq!(result.status, ImplementationStatus::Completed);
    assert!(result.pr_url.is_none());
    assert!(!permits_evidence_only_completion(
        "Implement a feature with pure verification."
    ));
}
