use super::*;

/// Production boundary: re-enter a persisted, verified task after its allocated
/// clone is changed to detached HEAD. The invariant is that the task is blocked
/// with its original clone and approved intent preserved, and it neither
/// publishes nor rewrites the human checkout or remote refs. Unit tests cannot
/// prove saved-state loading, clone validation and publication guards together.
/// Uses only a local Git repository, bare remote and fake provider; no Docker or
/// network, with one temporary repository setup.
#[test]
fn detached_task_clone_blocks_resume_without_retargeting_or_publication() {
    let sandbox = Sandbox::new();
    let started = super::run_on_worker(&sandbox, &sandbox.ticket).unwrap();
    assert_eq!(started.status, ImplementationStatus::AwaitingApproval);
    let pinned_head = started.verified_head.clone().unwrap();
    assert_eq!(
        sandbox.git(&started.task_repository, &["rev-parse", "HEAD"]),
        pinned_head
    );

    let state_dir = state_dir(&sandbox.planning_store.root, &sandbox.ticket).unwrap();
    let intent_before = sandbox.planning_store.read(&sandbox.ticket).unwrap();
    let remote_refs_before = remote_refs(&sandbox);
    let checkout_head_before = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    let checkout_branch_before = sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]);
    let checkout_status_before = sandbox.git(
        &sandbox.repo,
        &["status", "--porcelain", "--untracked-files=all"],
    );

    sandbox.git(
        &started.task_repository,
        &["checkout", "--detach", &pinned_head],
    );
    assert_eq!(
        sandbox.git(&started.task_repository, &["rev-parse", "HEAD"]),
        pinned_head
    );
    assert_eq!(
        sandbox.git(
            &started.task_repository,
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "HEAD"
    );

    let error = super::run_on_worker(&sandbox, &sandbox.ticket)
        .expect_err("detached task clone must not be resumed or published")
        .to_string();
    let detail =
        crate::core::implementation::load(&sandbox.planning_store.root, &sandbox.ticket).unwrap();
    assert_eq!(detail.status, ImplementationStatus::Blocked);
    assert!(
        detail.detail.to_lowercase().contains("symbolic ref"),
        "{error}: {}",
        detail.detail
    );
    assert!(
        error.to_lowercase().contains("symbolic ref") || error.to_lowercase().contains("branch"),
        "expected an actionable detached-HEAD error, got: {error}"
    );

    assert!(
        started.task_repository.exists(),
        "the original clone is preserved"
    );
    assert_eq!(detail.task_repository, started.task_repository);
    assert!(detail.task_repositories.contains(&started.task_repository));
    assert_eq!(detail.task_uid, started.task_uid);
    assert_eq!(detail.repository_id, started.repository_id);
    assert_eq!(detail.source_ref, started.source_ref);
    assert_eq!(detail.source_commit, started.source_commit);
    assert_eq!(detail.destination_branch, started.destination_branch);
    assert_eq!(detail.branch, started.branch);
    assert_eq!(detail.verified_head, started.verified_head);
    assert_eq!(
        sandbox.planning_store.read(&sandbox.ticket).unwrap(),
        intent_before
    );
    assert!(state_dir.join("state.json").is_file());
    assert!(!sandbox.root.join("pr-created").exists());
    assert_eq!(remote_refs(&sandbox), remote_refs_before);
    assert_eq!(
        sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]),
        checkout_head_before
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]),
        checkout_branch_before
    );
    assert_eq!(
        sandbox.git(
            &sandbox.repo,
            &["status", "--porcelain", "--untracked-files=all"]
        ),
        checkout_status_before
    );
}

fn remote_refs(sandbox: &Sandbox) -> String {
    sandbox.git(
        &sandbox.root,
        &[
            "--git-dir",
            sandbox.root.join("remote.git").to_str().unwrap(),
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ],
    )
}
