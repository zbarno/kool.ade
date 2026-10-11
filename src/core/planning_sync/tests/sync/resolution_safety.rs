use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningConflictChoice, PlanningSyncState, resolve_conflict_at,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn local_resolution_rebases_later_edits_and_preserves_the_latest_worktree_value() {
    let fixture = Fixture::new();
    let path = work_path(fixture.work_id);
    fixture.write_work(&fixture.first, fixture.work_id, "title", "remote value");
    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "title",
        "first local value",
    );
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "publish remote value",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "queue first local value",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();

    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "title",
        "latest local value",
    );
    let local_path = fixture.second.join(&path);
    let mut latest: serde_json::Value =
        serde_json::from_slice(&fs::read(&local_path).unwrap()).unwrap();
    latest["revision"] = 3.into();
    fs::write(&local_path, serde_json::to_vec_pretty(&latest).unwrap()).unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "queue latest local value",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();

    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let conflict = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(conflict.state, PlanningSyncState::Conflicted);
    let operation_id = conflict.conflicts[0].operation_id;

    let resolved = resolve_conflict_at(
        &fixture.second,
        &fixture.state_root("b"),
        operation_id,
        &path,
        PlanningConflictChoice::Local,
        None,
    )
    .unwrap();

    assert_eq!(resolved.state, PlanningSyncState::Synced, "{resolved:?}");
    let remote = fixture.remote_record(fixture.work_id);
    assert_eq!(remote["data"]["title"], "latest local value");
    assert_eq!(remote["revision"], 4);
    let local: serde_json::Value = serde_json::from_slice(&fs::read(local_path).unwrap()).unwrap();
    assert_eq!(local["data"]["title"], "latest local value");
    assert_eq!(local["revision"], 4);
}

#[test]
fn conflict_resolution_preserves_local_edits_made_after_conflict_detection() {
    let fixture = Fixture::new();
    let path = work_path(fixture.work_id);
    fixture.write_work(&fixture.first, fixture.work_id, "title", "remote value");
    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "title",
        "conflicting value",
    );
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "publish remote value",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();
    let operation_id = prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "publish local value",
        std::slice::from_ref(&path),
    )
    .unwrap()
    .unwrap();
    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let conflict = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(conflict.conflicts[0].operation_id, operation_id);

    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "title",
        "newer manual edit",
    );
    let error = resolve_conflict_at(
        &fixture.second,
        &fixture.state_root("b"),
        operation_id,
        &path,
        PlanningConflictChoice::Remote,
        None,
    )
    .unwrap_err();

    assert!(format!("{error:#}").contains("local record changed"));
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.second.join(path)).unwrap()).unwrap();
    assert_eq!(local["data"]["title"], "newer manual edit");
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "remote value"
    );
}
