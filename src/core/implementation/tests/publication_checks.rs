use super::*;

#[path = "publication_checks/provider_checks.rs"]
mod provider_checks;
#[path = "publication_checks/untracked_files.rs"]
mod untracked_files;

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
    assert!(!result.task_repository.join("unrelated.txt").exists());
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
    assert_eq!(held.status, ImplementationStatus::AwaitingApproval);
    assert!(held.pr_url.is_none() && held.merged_commit.is_none());
    assert!(held.task_repository.exists());
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
fn pr_creation_failure_moves_the_approved_task_to_actionable_attention() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let held = s
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    assert_eq!(held.status, ImplementationStatus::AwaitingApproval);

    fs::write(s.root.join("offline"), "").unwrap();
    let error = s
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap_err();
    let failure = Failure::from_error(&error);
    let blocked = record_failed_attempt(&s.repo, &s.ticket, &failure.message)
        .unwrap()
        .unwrap();

    assert_eq!(blocked.status, ImplementationStatus::Blocked);
    assert_eq!(board_column(Some(&blocked), false), 3);
    assert!(blocked.detail.contains("GitHub unavailable"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
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
    assert!(!result.task_repository.exists());
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
