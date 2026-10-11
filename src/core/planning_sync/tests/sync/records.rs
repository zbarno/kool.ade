use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn different_record_ids_survive_two_operator_pushes() {
    let fixture = Fixture::new();
    let other_id = uuid::Uuid::new_v4();
    fs::create_dir_all(fixture.first.join("state/work")).unwrap();
    fs::write(
        fixture.first.join(work_path(other_id)),
        super::super::support::record(other_id, "second", "local", 1),
    )
    .unwrap();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "operator a");
    fixture.write_work(&fixture.second, fixture.work_id, "detail", "operator b");
    let first_paths = vec![work_path(fixture.work_id), work_path(other_id)];
    let second_paths = vec![work_path(fixture.work_id)];
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "first",
        &first_paths,
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "second",
        &second_paths,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        sync_with_state_root(&fixture.first, &fixture.state_root("a"))
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );
    let status = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(status.state, PlanningSyncState::Synced);
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "operator a"
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["detail"],
        "operator b"
    );
    assert_eq!(fixture.remote_record(other_id)["data"]["title"], "second");
    let local_merged: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.second.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local_merged["data"]["title"], "operator a");
    assert_eq!(local_merged["data"]["detail"], "operator b");
    assert!(fixture.second.join(work_path(other_id)).is_file());
}

#[test]
fn manual_refresh_applies_remote_records_without_pending_local_edits() {
    let fixture = Fixture::new();
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "published remotely",
    );
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "publish",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        sync_with_state_root(&fixture.first, &fixture.state_root("a"))
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );
    let refreshed = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(refreshed.state, PlanningSyncState::Synced);
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.second.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local["data"]["title"], "published remotely");
}

#[test]
fn same_record_contradiction_is_retained_as_conflict() {
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
    let status = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(status.state, PlanningSyncState::Conflicted);
    assert_eq!(status.pending_operations, 1);
    assert_eq!(
        status.conflicts[0].record_id,
        fixture.work_id.hyphenated().to_string()
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "operator a"
    );
}

#[test]
fn contradictory_plan_approvals_do_not_overwrite_each_other() {
    let fixture = Fixture::new();
    for (repo, plan) in [(&fixture.first, "plan-a"), (&fixture.second, "plan-b")] {
        let path = repo.join(format!("state/workflow/{}.json", fixture.workflow_id));
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["data"]["approvedFeatures"]["FEATURE-1"]["approvedPlan"] = plan.into();
        record["revision"] = 2.into();
        record["updatedAtMs"] = 2.into();
        fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }
    prepare_local_intent(
        &fixture.first,
        &fixture.state_root("a"),
        "approve plan a",
        &[format!("state/workflow/{}.json", fixture.workflow_id)],
    )
    .unwrap()
    .unwrap();
    prepare_local_intent(
        &fixture.second,
        &fixture.state_root("b"),
        "approve plan b",
        &[format!("state/workflow/{}.json", fixture.workflow_id)],
    )
    .unwrap()
    .unwrap();
    sync_with_state_root(&fixture.first, &fixture.state_root("a")).unwrap();
    let status = sync_with_state_root(&fixture.second, &fixture.state_root("b")).unwrap();
    assert_eq!(status.state, PlanningSyncState::Conflicted);
    let remote: serde_json::Value = serde_json::from_slice(
        &super::super::support::git(
            None,
            &[
                "--git-dir",
                fixture.remote.to_str().unwrap(),
                "show",
                &format!(
                    "refs/heads/main:state/workflow/{}.json",
                    fixture.workflow_id
                ),
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(
        remote["data"]["approvedFeatures"]["FEATURE-1"]["approvedPlan"],
        "plan-a"
    );
}
