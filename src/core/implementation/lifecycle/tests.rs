use super::*;
use crate::core::{
    implementation::publication::lifecycle::tests::fixture::Fixture,
    lifecycle::{LifecycleStore, PullRequestPreparationStatus, TaskPhase},
};
use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

/// Production boundary: destination-head invalidation hashes the integrated Git diff,
/// persists the new head, and removes prior review, QA, and spec receipts before PR
/// authorization. A reducer test cannot prove the Git evidence is bound to this event.
/// Uses a temporary local Git repository and lifecycle store; no provider, Docker, model,
/// or browser. Setup cost: one repository, two local commits, and a fixture store.
#[test]
fn integrated_destination_head_invalidates_prior_lifecycle_receipts() {
    let fixture = Fixture::new(40);
    let mut implementation = fixture.implementation();
    let repository = &implementation.task_repository;
    git(repository, &["add", "-A"]);
    git(
        repository,
        &["commit", "-m", "fixture publication baseline"],
    );
    let base = git(repository, &["rev-parse", "HEAD"]);
    let changed_path = repository.join("integrated-change.txt");
    std::fs::write(&changed_path, "destination integration changed this task\n").unwrap();
    git(repository, &["add", "integrated-change.txt"]);
    git(
        repository,
        &["commit", "-m", "integrate destination update"],
    );
    let integrated_head = git(repository, &["rev-parse", "HEAD"]);
    let diff = git_bytes(
        repository,
        &["diff", "--binary", &format!("{base}...{integrated_head}")],
    );
    let expected_hash = format!("{:x}", Sha256::digest(diff));

    implementation.base_commit = base;
    implementation.verified_head = Some(integrated_head.clone());
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let state_dir = tempfile::tempdir().unwrap();

    assert!(
        reopen_if_publication_head_changed(
            &fixture.store,
            state_dir.path(),
            &implementation,
            &runner,
        )
        .unwrap()
    );

    let lifecycle = LifecycleStore::new(&fixture.store);
    let task = lifecycle.load_task(&fixture.task.task_uid).unwrap();
    assert_eq!(task.phase, TaskPhase::ReReview);
    assert_eq!(task.reviewed_code_head_sha, None);
    assert_eq!(
        task.implementation_sha256.as_deref(),
        Some(expected_hash.as_str())
    );
    assert!(task.code_review.is_none());
    assert!(task.runtime_qa.is_none());
    assert!(task.qa_execution.is_none());
    assert!(task.spec_reconciliation.is_none());
    assert_eq!(
        task.pr_work_item_uid.as_deref(),
        Some(fixture.work_uid.as_str())
    );

    let work = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
    assert_eq!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
    );
    assert!(work.provider_pr_url.is_none());
}

fn git(repository: &std::path::Path, args: &[&str]) -> String {
    String::from_utf8(git_bytes(repository, args))
        .unwrap()
        .trim()
        .to_owned()
}

fn git_bytes(repository: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
        ])
        .args(args)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
