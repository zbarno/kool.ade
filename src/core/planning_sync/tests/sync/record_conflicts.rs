use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningConflictChoice, PlanningSyncState, resolve_conflict_at,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn resolving_a_conflict_to_local_creates_a_new_validated_remote_operation() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "operator a");
    fixture.write_work(&fixture.second, fixture.work_id, "title", "operator b");
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "first",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "second",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let conflict = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    let operation_id = conflict.conflicts[0].operation_id;

    let resolved = resolve_conflict_at(
        &fixture.second,
        &fixture.state_root("b"),
        operation_id,
        &work_path(fixture.work_id),
        PlanningConflictChoice::Local,
        None,
    )
    .unwrap();

    assert_eq!(resolved.state, PlanningSyncState::Synced, "{resolved:?}");
    assert!(resolved.conflicts.is_empty());
    assert!(resolved.pending.is_empty());
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "operator b"
    );
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.second.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local["revision"], 3);
}

#[test]
fn resolving_a_conflict_to_remote_restores_the_confirmed_remote_record() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "operator a");
    fixture.write_work(&fixture.second, fixture.work_id, "title", "operator b");
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "first",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "second",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let conflict = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    let operation_id = conflict.conflicts[0].operation_id;

    let resolved = resolve_conflict_at(
        &fixture.second,
        &fixture.state_root("b"),
        operation_id,
        &work_path(fixture.work_id),
        PlanningConflictChoice::Remote,
        None,
    )
    .unwrap();

    assert_eq!(resolved.state, PlanningSyncState::Synced, "{resolved:?}");
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "operator a"
    );
    let journal_path = fixture
        .state_root("b")
        .join("projects")
        .join(fixture.workspace_id.hyphenated().to_string())
        .join("sync-operations.json");
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
    let resolution_id = journal["resolutions"][0]["resolutionId"].as_str().unwrap();
    let resolution: serde_json::Value = fixture.remote_resolution(resolution_id.parse().unwrap());
    assert_eq!(resolution["uid"], resolution_id);
    assert_eq!(
        resolution["data"]["sourceOperationId"],
        operation_id.to_string()
    );
    assert_eq!(resolution["data"]["choice"], "remote");
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.second.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local["data"]["title"], "operator a");
}

#[test]
fn resolving_a_conflict_with_manual_record_text_publishes_the_validated_record() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "operator a");
    fixture.write_work(&fixture.second, fixture.work_id, "title", "operator b");
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "first",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "second",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let conflict = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    let operation_id = conflict.conflicts[0].operation_id;
    let manual = serde_json::json!({
        "schemaVersion": 1,
        "uid": fixture.work_id.hyphenated().to_string(),
        "revision": 2,
        "createdAtMs": 1,
        "updatedAtMs": 2,
        "data": {"uid": fixture.work_id.hyphenated().to_string(), "title": "manual", "detail": "combined"}
    })
    .to_string();

    let resolved = resolve_conflict_at(
        &fixture.second,
        &fixture.state_root("b"),
        operation_id,
        &work_path(fixture.work_id),
        PlanningConflictChoice::Manual,
        Some(&manual),
    )
    .unwrap();

    assert_eq!(resolved.state, PlanningSyncState::Synced);
    let remote = fixture.remote_record(fixture.work_id);
    assert_eq!(remote["data"]["title"], "manual");
    assert_eq!(remote["data"]["detail"], "combined");
    assert_eq!(remote["revision"], 3);
}

#[test]
fn resolving_one_conflict_does_not_hide_an_unjournaled_commit_to_another_record() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("resolution-keeps-external-commit");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );
    fixture.write_work(&fixture.first, fixture.work_id, "title", "operator a");
    super::super::support::commit_all(&fixture.first, "local conflicting record");
    fixture.write_work(&fixture.second, fixture.work_id, "title", "operator b");
    super::super::support::commit_all(&fixture.second, "remote conflicting record");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );
    let operation_id = prepare_local_intent(
        &fixture.first,
        &state_root,
        "first operator",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    crate::core::planning_sync::runner::finish_shared_intent_at(
        &fixture.first,
        &state_root,
        operation_id,
    );
    let conflict = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(conflict.state, PlanningSyncState::Conflicted);

    let other_id = uuid::Uuid::new_v4();
    let other_path = work_path(other_id);
    fs::create_dir_all(fixture.first.join(&other_path).parent().unwrap()).unwrap();
    fs::write(
        fixture.first.join(&other_path),
        super::super::support::record(other_id, "external record", "unpublished", 1),
    )
    .unwrap();
    super::super::support::commit_all(&fixture.first, "external unjournaled record");

    let resolved = resolve_conflict_at(
        &fixture.first,
        &state_root,
        operation_id,
        &work_path(fixture.work_id),
        PlanningConflictChoice::Remote,
        None,
    )
    .unwrap();

    assert_eq!(resolved.state, PlanningSyncState::Pending, "{resolved:?}");
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "operator b"
    );
    let confirmed = resolved.confirmed_commit.as_deref().unwrap();
    assert_eq!(
        crate::core::planning_sync::git::read_path(&fixture.first, confirmed, &other_path).unwrap(),
        None,
        "external record must stay unpublished"
    );
}
