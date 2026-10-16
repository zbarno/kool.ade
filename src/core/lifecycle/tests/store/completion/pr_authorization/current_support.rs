use crate::core::lifecycle::LifecycleStore;
use std::path::Path;

#[allow(clippy::too_many_arguments)] // Keeps each checked merge-proof field explicit in this fixture.
pub(super) fn assert_current_evidence(
    lifecycle: &LifecycleStore<'_>,
    store: &crate::artifacts::planning_store::PlanningStore,
    task_uid: &str,
    feature_uid: &str,
    base_commit: &str,
    pr_head: &str,
    merge_commit: &str,
    repository: &Path,
) {
    let current = lifecycle
        .record_verified_current_implementation_for_local_fixture(
            task_uid,
            "github.com/example/project",
            repository,
        )
        .unwrap();
    assert_eq!(current.task_uid, task_uid);
    assert_eq!(current.feature_uid, feature_uid);
    assert_eq!(current.base_commit_sha, base_commit);
    assert_eq!(current.pull_request_head_sha, pr_head);
    assert_eq!(current.merge_commit_sha, merge_commit);
    assert_eq!(current.checked_out_head_sha, merge_commit);
    assert_eq!(
        lifecycle.load_current_implementation(feature_uid).unwrap(),
        Some(current)
    );
    let work_uid = lifecycle
        .load_task(task_uid)
        .unwrap()
        .pr_work_item_uid
        .unwrap();
    assert_stale_source_change_rejects_promotion(lifecycle, store, task_uid, &work_uid, repository);
}

pub(super) fn assert_merge_trigger(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    task_uid: &str,
    merged_revision: u64,
) {
    let trigger = lifecycle
        .list_spec_sync_queue(feature_uid, false)
        .unwrap()
        .into_iter()
        .filter(|entry| {
            entry.source_task_uid.as_deref() == Some(task_uid)
                && entry.trigger == crate::core::lifecycle::SpecSyncTriggerKindV1::PullRequest
        })
        .max_by_key(|entry| entry.source_revision)
        .expect("PR merge writes its reconciliation trigger");
    assert_eq!(trigger.source_revision, merged_revision);
}

pub(super) fn init_merged_repo(root: &Path) -> (String, String, String) {
    use std::process::Command;
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
    };
    let output = |args: &[&str]| {
        String::from_utf8(
            Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Synthetic Test"]);
    git(&["config", "user.email", "synthetic@example.invalid"]);
    std::fs::write(root.join("base.txt"), "base\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let base = output(&["rev-parse", "HEAD"]);
    git(&["checkout", "-qb", "feature"]);
    std::fs::write(root.join("feature.txt"), "feature\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "feature"]);
    let head = output(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "-B", "main", &base]);
    git(&["merge", "--no-ff", "-qm", "merge", "feature"]);
    let merged = output(&["rev-parse", "HEAD"]);
    git(&["init", "--bare", "-q", ".origin.git"]);
    git(&["remote", "add", "origin", ".origin.git"]);
    git(&["push", "-q", "origin", "feature:refs/pull/42/head"]);
    git(&["branch", "-D", "feature"]);
    (base, head, merged)
}

pub(super) fn pr_state(
    status: crate::core::lifecycle::PullRequestPreparationStatus,
    base: &str,
    head: &str,
    merged: Option<&str>,
) -> crate::core::lifecycle::PullRequestPublicationState {
    crate::core::lifecycle::PullRequestPublicationState {
        status,
        provider_pr_id: Some(42),
        provider_pr_url: Some("https://github.com/example/project/pull/42".into()),
        head_sha: Some(head.into()),
        base_sha: Some(base.into()),
        merged_commit_sha: merged.map(str::to_owned),
    }
}

pub(super) fn assert_stale_source_change_rejects_promotion(
    lifecycle: &LifecycleStore<'_>,
    store: &crate::artifacts::planning_store::PlanningStore,
    task_uid: &str,
    work_uid: &str,
    repository: &Path,
) {
    use std::process::Command;
    assert!(
        Command::new("git")
            .args(["commit", "--allow-empty", "-qm", "advance checkout"])
            .current_dir(repository)
            .status()
            .unwrap()
            .success()
    );
    let path = format!("state/lifecycle_pr_work_items/{work_uid}.json");
    let (original, revision) = store.read_record::<serde_json::Value>(&path).unwrap();
    let mut changed = original.clone();
    changed["revision"] = (revision + 1).into();
    changed["record"]["revision"] = (revision + 1).into();
    let result = lifecycle.record_verified_current_implementation_for_local_fixture_with_precommit(
        task_uid,
        "github.com/example/project",
        repository,
        || {
            store.save_record(&path, &changed, revision).unwrap();
        },
    );
    assert!(matches!(
        result,
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision {
            path: ref stale_path,
            ..
        }) if stale_path == &path
    ));
    store
        .transaction(
            &[(path, serde_json::to_vec_pretty(&original).unwrap())],
            None,
        )
        .unwrap();
}
