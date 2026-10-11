use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn pending_operations_without_remote_identity_require_destination_confirmation() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("legacy-remote-identity");
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "pending legacy operation",
    );
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "pending legacy operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let journal_path = state_root
        .join("projects")
        .join(fixture.workspace_id.hyphenated().to_string())
        .join("sync-operations.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal.as_object_mut().unwrap().remove("remoteIdentity");
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    let second_remote = fixture.root.join("legacy-second-remote.git");
    super::super::support::git(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--bare",
            second_remote.to_str().unwrap(),
        ],
    );
    super::super::support::git(
        Some(&fixture.first),
        &[
            "remote",
            "set-url",
            "origin",
            second_remote.to_str().unwrap(),
        ],
    );

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending);
    assert_eq!(status.pending_operations, 1);
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("no recorded remote identity")
    );
    assert!(
        fs::read_dir(second_remote.join("refs/heads"))
            .unwrap()
            .next()
            .is_none(),
        "pending operations must not be published to an unconfirmed destination"
    );
}

#[test]
fn legacy_synced_journal_refreshes_remote_changes_without_staying_pending() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("legacy-local-checkpoint");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );

    let path = work_path(fixture.work_id);
    fixture.write_work(&fixture.first, fixture.work_id, "title", "published title");
    let operation_id = prepare_local_intent(
        &fixture.first,
        &state_root,
        "publish title",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();
    super::super::support::commit_all(&fixture.first, "local application checkpoint");
    crate::core::planning_sync::runner::finish_shared_intent_at(
        &fixture.first,
        &state_root,
        operation_id,
    );
    let published = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(published.state, PlanningSyncState::Synced);
    let local_head = crate::core::planning_sync::git::head(&fixture.first).unwrap();
    assert_ne!(
        published.confirmed_commit.as_deref(),
        Some(local_head.as_str())
    );

    let journal_path = state_root
        .join("projects")
        .join(fixture.workspace_id.hyphenated().to_string())
        .join("sync-operations.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal
        .as_object_mut()
        .unwrap()
        .remove("confirmedLocalCommit");
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    super::super::support::git(
        Some(&fixture.second),
        &["pull", "--ff-only", "origin", "main"],
    );
    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "title",
        "remote successor title",
    );
    super::super::support::commit_all(&fixture.second, "remote successor title");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let refreshed = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(refreshed.state, PlanningSyncState::Synced, "{refreshed:?}");
    let worktree: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.first.join(&path)).expect("remote refresh should update worktree"),
    )
    .unwrap();
    assert_eq!(worktree["data"]["title"], "remote successor title");
}

#[test]
fn legacy_journal_keeps_unpublished_commit_pending_after_remote_refresh() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("legacy-unpublished-commit");
    let initial = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(initial.state, PlanningSyncState::Synced);
    let confirmed = initial.confirmed_commit.unwrap();
    let path = work_path(fixture.work_id);

    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "unpublished local title",
    );
    super::super::support::commit_all(&fixture.first, "unpublished local title");
    let confirmed_content =
        crate::core::planning_sync::git::read_path(&fixture.first, &confirmed, &path)
            .unwrap()
            .unwrap();
    fs::write(fixture.first.join(&path), confirmed_content).unwrap();

    let journal_path = state_root
        .join("projects")
        .join(fixture.workspace_id.hyphenated().to_string())
        .join("sync-operations.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal
        .as_object_mut()
        .unwrap()
        .remove("confirmedLocalCommit");
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    fixture.write_work(&fixture.second, fixture.work_id, "title", "remote title");
    super::super::support::commit_all(&fixture.second, "remote title");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending, "{status:?}");
    let worktree: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(&path)).unwrap()).unwrap();
    assert_eq!(worktree["data"]["title"], "remote title");
}
