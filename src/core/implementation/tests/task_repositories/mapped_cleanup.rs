use super::*;

#[path = "mapped_cleanup/fixture.rs"]
mod fixture;
use fixture::{
    confirm_merges_and_fast_forward, publish_tasks, refs, refs_after_merge, setup_api_repository,
    start_tasks,
};

/// Production boundary: execute tasks concurrently against root and mapped API
/// repositories, publish their PRs sequentially to distinct targets, then run
/// provider-confirmed merge refresh and clone cleanup with a third task active.
/// Unit routing checks cannot prove pushes, PR identities, merge refs, and clone
/// ownership agree. Local Git/bare remotes plus synthetic `gh`/SSH; no Docker or
/// network. The operator checkouts advance only as the approved merges land.
#[test]
fn concurrent_mapped_tasks_publish_scoped_prs_and_cleanup_only_owned_clones() {
    let sandbox = Sandbox::new();
    let root_remote = sandbox.root.join("remote.git");
    let root_refs_before = refs(&root_remote);
    let api_repository = setup_api_repository(&sandbox);
    let api_ticket = "planning/tasks/feature/002-api-mapped.md";
    let unrelated_ticket = "planning/tasks/feature/003-unrelated.md";
    let active = start_tasks(&sandbox, &api_repository.source_sha);

    assert_eq!(active.root.repository_id.as_deref(), Some("root"));
    assert_eq!(active.api.repository_id.as_deref(), Some("api"));
    assert_eq!(active.unrelated.repository_id.as_deref(), Some("root"));
    for state in [&active.root, &active.api, &active.unrelated] {
        assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
        assert!(state.task_repository.join(".git").is_dir());
        assert_eq!(
            sandbox.git(&state.task_repository, &["branch", "--show-current"]),
            state.branch
        );
        assert_eq!(
            sandbox.git(&state.task_repository, &["rev-parse", "HEAD"]),
            state.verified_head.as_deref().unwrap()
        );
    }
    assert_ne!(active.root.task_repository, active.api.task_repository);
    assert_ne!(
        active.root.task_repository,
        active.unrelated.task_repository
    );
    assert_ne!(active.api.task_repository, active.unrelated.task_repository);
    assert_eq!(active.root_calls.load(Ordering::SeqCst), 1);
    assert_eq!(active.api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(active.unrelated_calls.load(Ordering::SeqCst), 1);

    let published = publish_tasks(&sandbox, &root_remote, &api_repository, &active);
    let checkout =
        confirm_merges_and_fast_forward(&sandbox, &root_remote, &api_repository, &active);
    for ticket in [&sandbox.ticket, api_ticket] {
        crate::core::implementation::refresh_pr_with_store_for_test(
            &sandbox.planning_store,
            &sandbox.planning_store.root,
            &sandbox.repo,
            ticket,
            sandbox.gh.to_str().unwrap(),
        )
        .unwrap();
    }

    for state in [&published.root, &published.api] {
        let cleaned =
            crate::core::implementation::load(&sandbox.planning_store.root, &state.ticket).unwrap();
        assert_eq!(cleaned.status, ImplementationStatus::Completed);
        assert!(
            cleaned.cleanup.completed_at.is_some(),
            "{:?}",
            cleaned.cleanup
        );
        assert!(
            !state.task_repository.exists(),
            "merged task clone should be reclaimed"
        );
    }
    assert!(active.unrelated.task_repository.exists());
    let unrelated =
        crate::core::implementation::load(&sandbox.planning_store.root, unrelated_ticket).unwrap();
    assert_eq!(unrelated.task_uid, active.unrelated.task_uid);
    assert_eq!(unrelated.task_repository, active.unrelated.task_repository);
    assert_eq!(unrelated.status, ImplementationStatus::AwaitingApproval);

    assert_eq!(
        sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]),
        checkout.root_head
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]),
        checkout.root_branch
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["status", "--porcelain"]),
        checkout.root_status
    );
    assert_eq!(
        sandbox.git(&api_repository.checkout, &["rev-parse", "HEAD"]),
        checkout.api_head
    );
    assert_eq!(
        sandbox.git(
            &api_repository.checkout,
            &["symbolic-ref", "--short", "HEAD"]
        ),
        checkout.api_branch
    );
    assert_eq!(
        sandbox.git(&api_repository.checkout, &["status", "--porcelain"]),
        checkout.api_status
    );
    assert_eq!(
        refs(&root_remote),
        refs_after_merge(&root_refs_before, &active.root, "main")
    );
    assert_eq!(
        refs(&api_repository.remote),
        refs_after_merge(&api_repository.refs_before, &active.api, "stable")
    );
}
