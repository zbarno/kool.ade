use super::super::*;
use super::{repositories::ApiRepository, tasks::ActiveTasks};

pub struct CheckoutSnapshot {
    pub root_head: String,
    pub root_branch: String,
    pub root_status: String,
    pub api_head: String,
    pub api_branch: String,
    pub api_status: String,
}

pub fn confirm_merges_and_fast_forward(
    sandbox: &Sandbox,
    root_remote: &Path,
    api_repository: &ApiRepository,
    active: &ActiveTasks,
) -> CheckoutSnapshot {
    let root_head = active.root.verified_head.as_deref().unwrap();
    let api_head = active.api.verified_head.as_deref().unwrap();
    sandbox.git(root_remote, &["update-ref", "refs/heads/main", root_head]);
    sandbox.git(root_remote, &["update-ref", "refs/pull/1/head", root_head]);
    sandbox.git(
        &api_repository.remote,
        &["update-ref", "refs/heads/stable", api_head],
    );
    sandbox.git(
        &api_repository.remote,
        &["update-ref", "refs/pull/1/head", api_head],
    );
    fs::write(sandbox.root.join("merged-root"), root_head).unwrap();
    fs::write(sandbox.root.join("merged-api"), api_head).unwrap();

    sandbox.git(&sandbox.repo, &["fetch", "-q", "origin"]);
    sandbox.git(&sandbox.repo, &["merge", "--ff-only", root_head]);
    sandbox.git(&api_repository.checkout, &["fetch", "-q", "origin"]);
    sandbox.git(&api_repository.checkout, &["checkout", "-q", "stable"]);
    sandbox.git(&api_repository.checkout, &["merge", "--ff-only", api_head]);
    let snapshot = CheckoutSnapshot {
        root_head: sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]),
        root_branch: sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]),
        root_status: sandbox.git(&sandbox.repo, &["status", "--porcelain"]),
        api_head: sandbox.git(&api_repository.checkout, &["rev-parse", "HEAD"]),
        api_branch: sandbox.git(
            &api_repository.checkout,
            &["symbolic-ref", "--short", "HEAD"],
        ),
        api_status: sandbox.git(&api_repository.checkout, &["status", "--porcelain"]),
    };
    assert_eq!(snapshot.root_head, root_head);
    assert_eq!(snapshot.api_head, api_head);
    snapshot
}
