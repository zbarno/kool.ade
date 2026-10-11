use super::super::support::{Fixture, work_path};
use crate::core::managed_workspace::WorkspaceManager;
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn changing_the_planning_remote_invalidates_confirmation_but_equivalent_urls_do_not() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("remote-identity");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager.join(&fixture.first).unwrap();
    assert_eq!(
        manager.sync_planning(workspace.id).unwrap().state,
        PlanningSyncState::Synced
    );

    let equivalent = format!("{}/", fixture.remote.display());
    manager
        .set_planning_remote(workspace.id, &equivalent)
        .unwrap();
    let equivalent_status = manager.planning_sync_status(workspace.id).unwrap();
    assert_eq!(equivalent_status.state, PlanningSyncState::Synced);
    assert!(equivalent_status.confirmed_commit.is_some());

    let second_remote = fixture.root.join("second-remote.git");
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
    manager
        .set_planning_remote(workspace.id, &second_remote.to_string_lossy())
        .unwrap();
    let changed_status = manager.planning_sync_status(workspace.id).unwrap();
    assert_eq!(changed_status.state, PlanningSyncState::Pending);
    assert!(changed_status.confirmed_commit.is_none());
    assert!(changed_status.confirmed_at_ms.is_none());
    assert!(
        changed_status
            .detail
            .as_deref()
            .unwrap()
            .contains("remote changed")
    );
}

#[test]
fn changing_the_planning_remote_is_rejected_while_operations_are_pending() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("pending-remote-change");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager.join(&fixture.first).unwrap();
    assert_eq!(
        manager.sync_planning(workspace.id).unwrap().state,
        PlanningSyncState::Synced
    );

    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "queued planning change",
    );
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "queued planning change",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let original_remote = String::from_utf8(
        super::super::support::git(Some(&fixture.first), &["remote", "get-url", "origin"]).stdout,
    )
    .unwrap();

    let second_remote = fixture.root.join("second-pending-remote.git");
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
    let error = manager
        .set_planning_remote(workspace.id, &second_remote.to_string_lossy())
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("operations or conflicts are pending")
    );
    assert_eq!(
        String::from_utf8(
            super::super::support::git(Some(&fixture.first), &["remote", "get-url", "origin"])
                .stdout,
        )
        .unwrap(),
        original_remote
    );
    let status = manager.planning_sync_status(workspace.id).unwrap();
    assert_eq!(status.pending_operations, 1);
}

#[test]
fn sync_does_not_rebind_pending_operations_after_an_external_remote_change() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("external-pending-remote-change");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager.join(&fixture.first).unwrap();
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "queued planning change",
    );
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "queued planning change",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    let second_remote = fixture.root.join("second-external-remote.git");
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
            .contains("remote changed while local sync operations or conflicts are pending")
    );
    let error = manager
        .set_planning_remote(workspace.id, &second_remote.to_string_lossy())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("operations or conflicts are pending")
    );
}

#[test]
fn removing_origin_preserves_pending_status_and_exportability() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("missing-pending-remote");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager.join(&fixture.first).unwrap();
    assert_eq!(
        manager.sync_planning(workspace.id).unwrap().state,
        PlanningSyncState::Synced
    );
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "preserved without remote",
    );
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "preserved without remote",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    super::super::support::git(Some(&fixture.first), &["remote", "remove", "origin"]);

    let status = manager.planning_sync_status(workspace.id).unwrap();
    assert_eq!(status.state, PlanningSyncState::Pending);
    assert_eq!(status.pending_operations, 1);
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("remote is not configured")
    );
    let retry = manager.sync_planning(workspace.id).unwrap();
    assert_eq!(retry.state, PlanningSyncState::Pending);
    assert_eq!(retry.pending_operations, 1);

    let export = fixture.root.join("missing-remote-export.json");
    manager
        .export_pending_planning(workspace.id, &export)
        .unwrap();
    assert!(
        fs::read_to_string(export)
            .unwrap()
            .contains("preserved without remote")
    );
}
