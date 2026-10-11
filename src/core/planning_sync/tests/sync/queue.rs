use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn queued_edits_to_one_record_replay_in_order_and_refresh_the_latest_snapshot() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("queued-edits");
    let path = work_path(fixture.work_id);
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "first queued edit",
    );
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "first edit",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();

    let record_path = fixture.first.join(&path);
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record["data"]["detail"] = "second queued edit".into();
    record["revision"] = 3.into();
    record["updatedAtMs"] = 3.into();
    fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    prepare_local_intent(&fixture.first, &state_root, "second edit", &[path])
        .unwrap()
        .unwrap();

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Synced);
    assert_eq!(status.pending_operations, 0);
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "first queued edit"
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["detail"],
        "second queued edit"
    );
}

#[test]
fn first_journaled_sync_includes_unpushed_changes_to_other_records() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("local-commit");
    let committed_id = uuid::Uuid::new_v4();

    fs::create_dir_all(fixture.first.join("state/work")).unwrap();
    fs::write(
        fixture.first.join(work_path(committed_id)),
        super::super::support::record(committed_id, "committed locally first", "stable", 1),
    )
    .unwrap();
    fixture.write_work(
        &fixture.first,
        committed_id,
        "title",
        "committed locally first",
    );
    super::super::support::commit_all(&fixture.first, "unrelated local planning checkpoint");
    let path = fixture.first.join(work_path(fixture.work_id));
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["data"]["detail"] = "journaled afterward".into();
    record["revision"] = 3.into();
    record["updatedAtMs"] = 3.into();
    fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

    prepare_local_intent(
        &fixture.first,
        &state_root,
        "journaled edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Synced);
    assert_eq!(
        fixture.remote_record(committed_id)["data"]["title"],
        "committed locally first"
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["detail"],
        "journaled afterward"
    );
}

#[test]
fn mismatched_remote_workspace_manifest_blocks_sync_before_push() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("wrong-remote");
    fixture.write_work(&fixture.first, fixture.work_id, "title", "must stay local");
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "local intent",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    let manifest_path = fixture.second.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspaceId"] = uuid::Uuid::new_v4().hyphenated().to_string().into();
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    super::super::support::commit_all(&fixture.second, "replace remote identity");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::IncompatibleSchema);
    assert_eq!(status.pending_operations, 1);
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "original"
    );
}

#[cfg(unix)]
#[test]
fn worktree_record_reader_rejects_symlinked_ancestor_directories() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let work_dir = fixture.first.join("state/work");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::copy(
        work_dir.join(format!("{}.json", fixture.work_id)),
        outside.join(format!("{}.json", fixture.work_id)),
    )
    .unwrap();
    fs::remove_dir_all(&work_dir).unwrap();
    symlink(outside, &work_dir).unwrap();

    let error = prepare_local_intent(
        &fixture.first,
        &fixture.state_root("symlink"),
        "refuse symlink",
        &[work_path(fixture.work_id)],
    )
    .unwrap_err();

    assert!(error.to_string().contains("symlink records"));
}
