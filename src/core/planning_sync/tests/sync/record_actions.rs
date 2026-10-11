use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState, cancel_pending_operation_at, export_pending_at,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn only_uncommitted_pending_operations_can_be_cancelled_and_export_is_no_overwrite() {
    let fixture = Fixture::new();
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "pending local edit",
    );
    let operation_id = prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "pending edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let pending =
        crate::core::planning_sync::runner::status_at(&fixture.first, &fixture.state_root("a"))
            .unwrap();
    assert_eq!(pending.pending[0].operation_id, operation_id);
    assert!(!pending.pending[0].has_local_commit);

    let export = fixture.root.join("pending-export.json");
    let exported = export_pending_at(&fixture.first, &fixture.state_root("a"), &export).unwrap();
    assert_eq!(exported.pending_operations, 1);
    assert!(export.exists());
    assert!(export_pending_at(&fixture.first, &fixture.state_root("a"), &export).is_err());

    let cancelled =
        cancel_pending_operation_at(&fixture.first, &fixture.state_root("a"), operation_id)
            .unwrap();
    assert_eq!(cancelled.pending_operations, 0);
    let restored: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(restored["data"]["title"], "original");
}

#[test]
fn cancellation_rejects_a_commit_written_before_its_journal_marker() {
    let fixture = Fixture::new();
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "committed pending edit",
    );
    let state_root = fixture.state_root("crash-before-marker");
    let operation_id = prepare_local_intent(
        &fixture.first,
        &state_root,
        "pending edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    super::super::support::commit_all(&fixture.first, "commit before journal marker");

    let error = cancel_pending_operation_at(&fixture.first, &state_root, operation_id).unwrap_err();

    assert!(format!("{error:#}").contains("only an uncommitted"));
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local["data"]["title"], "committed pending edit");
    let status =
        crate::core::planning_sync::runner::status_at(&fixture.first, &state_root).unwrap();
    assert_eq!(status.pending_operations, 1);
}

#[test]
fn cancellation_waits_for_the_repository_checkpoint_lock() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "pending edit");
    let state_root = fixture.state_root("cancel-lock");
    let operation_id = prepare_local_intent(
        &fixture.first,
        &state_root,
        "pending edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let lock = crate::core::gitops::lock_repository(&fixture.first).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let repo = fixture.first.clone();
    let worker_state_root = state_root.clone();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = cancel_pending_operation_at(&repo, &worker_state_root, operation_id);
        done_tx.send(result).unwrap();
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        done_rx.recv_timeout(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop(lock);
    done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
}

#[test]
fn removing_origin_blocks_shared_commits_until_the_remote_is_restored() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("originless-shared-commit");
    assert_eq!(
        crate::core::planning_sync::runner::sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        crate::core::planning_sync::PlanningSyncState::Synced
    );
    let path = work_path(fixture.work_id);
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "edit without origin",
    );
    let head_before = String::from_utf8(
        super::super::support::git(Some(&fixture.first), &["rev-parse", "HEAD"]).stdout,
    )
    .unwrap();
    super::super::support::git(Some(&fixture.first), &["remote", "remove", "origin"]);

    let error = crate::core::gitops::commit_planning_changes_at(
        &fixture.first,
        "must remain uncommitted",
        std::slice::from_ref(&path),
        &state_root,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning remote is not configured")
    );
    assert_eq!(
        String::from_utf8(
            super::super::support::git(Some(&fixture.first), &["rev-parse", "HEAD"]).stdout,
        )
        .unwrap(),
        head_before
    );
    assert!(fixture.first.join(&path).is_file());
    super::super::support::git(
        Some(&fixture.first),
        &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
    );

    crate::core::gitops::commit_planning_changes_at(
        &fixture.first,
        "commit after restoring origin",
        std::slice::from_ref(&path),
        &state_root,
    )
    .unwrap();

    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "edit without origin"
    );
    let status =
        crate::core::planning_sync::runner::status_at(&fixture.first, &state_root).unwrap();
    assert_eq!(
        status.state,
        crate::core::planning_sync::PlanningSyncState::Synced
    );
}

#[test]
fn published_local_commit_can_merge_a_concurrent_remote_field_without_staying_pending() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("concurrent-remote-field");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );

    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "detail",
        "concurrent remote detail",
    );
    super::super::support::commit_all(&fixture.second, "concurrent remote field");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let path = work_path(fixture.work_id);
    fixture.write_work(&fixture.first, fixture.work_id, "title", "local title");
    crate::core::gitops::commit_planning_changes_at(
        &fixture.first,
        "local title",
        std::slice::from_ref(&path),
        &state_root,
    )
    .unwrap();
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Synced, "{status:?}");
    let remote = fixture.remote_record(fixture.work_id);
    assert_eq!(remote["data"]["title"], "local title");
    assert_eq!(remote["data"]["detail"], "concurrent remote detail");
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(path)).unwrap()).unwrap();
    assert_eq!(local["data"]["detail"], "concurrent remote detail");
}
