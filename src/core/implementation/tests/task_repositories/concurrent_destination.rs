use super::*;
use crate::core::implementation::tests::publication_checks::authorize_fixture_publication;

#[path = "concurrent_destination/fixture.rs"]
mod fixture;
use fixture::{TaskChanges, remote_ref, remote_ref_exists, run_task};

/// Production boundary: three real task executions share one destination SHA;
/// PR A is published and merged into the local bare remote, then B's same-file
/// edit is reconciled and C's disjoint edit is re-integrated against the moved
/// destination. Unit tests of overlap detection cannot prove the integration
/// lock, destination refetch, task persistence, and verified clone agree. The
/// fixture uses local Git/bare remotes and fake `gh` only (no network/Docker),
/// with three task executions and two destination re-integrations.
#[test]
fn destination_advance_reconciles_overlapping_and_keeps_disjoint_task_changes() {
    let sandbox = Sandbox::new_with_initial_files(&[("implemented.txt", "implemented\n")]);
    let remote = sandbox.root.join("remote.git");
    let original_base = sandbox.git(&remote, &["rev-parse", "refs/heads/main"]);

    let second = "planning/tasks/feature/002-shared-file.md";
    super::sandbox::write_lifecycle_task_with_main_intent(
        &sandbox,
        second,
        "# Shared file task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-002",
        "Shared file task",
    );
    let third = "planning/tasks/feature/003-disjoint-file.md";
    super::sandbox::write_lifecycle_task_with_main_intent(
        &sandbox,
        third,
        "# Disjoint file task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-003",
        "Disjoint file task",
    );

    // All three tasks pin the exact same destination before any task is
    // integrated. The baseline `implemented.txt` satisfies the fixture's
    // generic verification command without creating a shared file conflict.
    let first_changes =
        TaskChanges::new(&[("shared.txt", "first edit\n"), ("first-only.txt", "A\n")]);
    let overlapping_changes =
        TaskChanges::new(&[("shared.txt", "second edit\n"), ("second-only.txt", "B\n")]);
    let disjoint_changes = TaskChanges::new(&[("third-only.txt", "C\n")]);
    let mut first = run_task(&sandbox, &sandbox.ticket, &first_changes).unwrap();
    let overlapping = run_task(&sandbox, second, &overlapping_changes).unwrap();
    let disjoint = run_task(&sandbox, third, &disjoint_changes).unwrap();

    for state in [&first, &overlapping, &disjoint] {
        assert_eq!(state.base_commit, original_base);
        assert_eq!(state.destination_branch.as_deref(), Some("main"));
        assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
        assert!(state.task_repository.join(".git").is_dir());
    }
    assert_ne!(first.task_uid, overlapping.task_uid);
    assert_ne!(first.task_uid, disjoint.task_uid);
    assert_ne!(overlapping.task_uid, disjoint.task_uid);
    assert_ne!(first.task_repository, overlapping.task_repository);
    assert_ne!(first.task_repository, disjoint.task_repository);
    assert_ne!(overlapping.task_repository, disjoint.task_repository);

    // A and B are authorized against their own verified heads. Publish A
    // through the real PR path, then simulate the human-approved
    // fast-forward merge by advancing only the test bare remote's main ref.
    authorize_fixture_publication(&sandbox, &first);
    authorize_fixture_publication(&sandbox, &overlapping);
    first = run_task(&sandbox, &sandbox.ticket, &first_changes).unwrap();
    assert_eq!(first.status, ImplementationStatus::AwaitingReview);
    let first_head = first.verified_head.clone().unwrap();
    assert_eq!(remote_ref(&sandbox, &first.branch), first_head);
    assert!(
        sandbox
            .git(
                &remote,
                &["merge-base", "--is-ancestor", &original_base, &first_head]
            )
            .is_empty()
    );
    sandbox.git(&remote, &["update-ref", "refs/heads/main", &first_head]);
    assert_eq!(remote_ref(&sandbox, "main"), first_head);
    let published_args = fs::read(sandbox.root.join("pr-args")).unwrap();
    let published_count = fs::read_to_string(sandbox.root.join("pr-created"))
        .unwrap()
        .lines()
        .count();

    // B's same-file conflict repair and C's disjoint implementation resume
    // begin after A advanced main. Their Implementation-mode harness calls
    // meet at a timed rendezvous, proving both workers overlap before their
    // destination integrations serialize under the repository lock.
    let rendezvous = Arc::new(fixture::ImplementationRendezvous::new(2));
    let overlapping_changes = TaskChanges::with_rendezvous(
        &[("shared.txt", "first edit\nsecond edit\n")],
        Arc::clone(&rendezvous),
    );
    let disjoint_changes =
        TaskChanges::with_rendezvous(&[("third-only.txt", "C\n")], Arc::clone(&rendezvous));
    let (overlapping, disjoint) = std::thread::scope(|scope| {
        let second_worker = scope.spawn(|| run_task(&sandbox, second, &overlapping_changes));
        let third_worker = scope.spawn(|| run_task(&sandbox, third, &disjoint_changes));
        (second_worker.join().unwrap(), third_worker.join().unwrap())
    });
    let overlapping = overlapping.unwrap();
    let disjoint = disjoint.unwrap();
    assert_eq!(rendezvous.arrivals(), 2);
    assert_eq!(disjoint.status, ImplementationStatus::AwaitingApproval);
    assert!(disjoint.pr_url.is_none());
    assert_eq!(disjoint.base_commit, first_head);
    assert_eq!(remote_ref(&sandbox, "main"), first_head);
    assert_eq!(overlapping.status, ImplementationStatus::AwaitingReview);
    assert!(overlapping.detail.contains("publication head differs"));
    assert!(
        overlapping
            .detail
            .contains("refresh code review, runtime QA")
    );
    assert!(overlapping.pr_url.is_none());
    assert_eq!(overlapping.base_commit, first_head);
    let overlapping_head = overlapping.verified_head.clone().unwrap();
    assert_eq!(
        sandbox.git(&overlapping.task_repository, &["rev-parse", "HEAD"]),
        overlapping_head
    );
    assert_eq!(
        fs::read_to_string(overlapping.task_repository.join("shared.txt")).unwrap(),
        "first edit\nsecond edit\n"
    );
    assert_eq!(
        fs::read_to_string(overlapping.task_repository.join("first-only.txt")).unwrap(),
        "A\n"
    );
    assert_eq!(
        fs::read_to_string(overlapping.task_repository.join("second-only.txt")).unwrap(),
        "B\n"
    );
    assert!(
        sandbox
            .git(
                &overlapping.task_repository,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &first_head,
                    &overlapping_head
                ]
            )
            .is_empty()
    );
    let saved_overlapping =
        crate::core::implementation::load(&sandbox.planning_store.root, second).unwrap();
    assert_eq!(saved_overlapping.task_uid, overlapping.task_uid);
    assert_eq!(saved_overlapping.base_commit, first_head);
    assert_eq!(
        saved_overlapping.verified_head,
        Some(overlapping_head.clone())
    );
    assert_eq!(
        saved_overlapping.task_repository,
        overlapping.task_repository
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&sandbox.planning_store);
    let task = lifecycle
        .task_for_planning_item(overlapping.task_uid.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_ne!(
        task.reviewed_code_head_sha.as_deref(),
        Some(overlapping_head.as_str())
    );
    let work = lifecycle
        .load_pr_preparation(task.pr_work_item_uid.as_deref().unwrap())
        .unwrap();
    assert!(work.provider_pr_url.is_none());
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        published_count
    );
    assert_eq!(
        fs::read(sandbox.root.join("pr-args")).unwrap(),
        published_args
    );
    assert!(!remote_ref_exists(&sandbox, &overlapping.branch));
    assert_eq!(remote_ref(&sandbox, "main"), first_head);

    // C has no overlapping paths with A. It still started from the stale base,
    // but production integration must rebase its verified change on A without
    // invoking conflict repair or losing either task's file.
    assert_eq!(
        sandbox.git(&disjoint.task_repository, &["rev-parse", "HEAD"]),
        disjoint.verified_head.as_deref().unwrap()
    );
    assert_eq!(
        fs::read_to_string(disjoint.task_repository.join("first-only.txt")).unwrap(),
        "A\n"
    );
    assert_eq!(
        fs::read_to_string(disjoint.task_repository.join("third-only.txt")).unwrap(),
        "C\n"
    );
    assert_eq!(
        fs::read_to_string(disjoint.task_repository.join("shared.txt")).unwrap(),
        "first edit\n"
    );
    let saved_disjoint =
        crate::core::implementation::load(&sandbox.planning_store.root, third).unwrap();
    assert_eq!(saved_disjoint.task_uid, disjoint.task_uid);
    assert_eq!(saved_disjoint.base_commit, first_head);
    assert_eq!(saved_disjoint.verified_head, disjoint.verified_head);
    assert_eq!(saved_disjoint.task_repository, disjoint.task_repository);
    assert!(!remote_ref_exists(&sandbox, &disjoint.branch));
    assert_eq!(remote_ref(&sandbox, "main"), first_head);
}
