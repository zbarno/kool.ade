use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};

#[test]
fn accepted_push_with_lost_ack_is_idempotent_after_restart() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "accepted");
    let state_root = fixture.state_root("a");
    let id = prepare_local_intent(
        &fixture.first,
        &state_root,
        "intent",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let journal = crate::core::planning_sync::journal::OperationJournal::load_or_new(
        &state_root,
        fixture.workspace_id,
        "refs/heads/main",
    )
    .unwrap();
    let op = journal.operations[0].clone();
    let remote_base =
        crate::core::planning_sync::git::fetch(&fixture.first, &op.canonical_ref).unwrap();
    let (mutations, _, conflicts) =
        crate::core::planning_sync::runner::reapply(&fixture.first, &remote_base, &op.mutations)
            .unwrap();
    assert!(conflicts.is_empty());
    let commit = crate::core::planning_sync::git::create_commit(
        &fixture.first,
        &remote_base,
        id,
        &op.message,
        &mutations,
    )
    .unwrap();
    crate::core::planning_sync::git::push(&fixture.first, &commit, &op.canonical_ref).unwrap();
    let restarted_status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(restarted_status.state, PlanningSyncState::Synced);
    assert_eq!(restarted_status.pending_operations, 0);
    let count = super::super::support::git(
        Some(&fixture.first),
        &[
            "log",
            "--format=%H",
            "--fixed-strings",
            "--grep",
            &format!("Koolade-Operation: {id}"),
            "refs/remotes/origin/main",
        ],
    );
    assert_eq!(
        String::from_utf8_lossy(&count.stdout).lines().count(),
        1,
        "operation marker should occur once in remote history: {}",
        String::from_utf8_lossy(&count.stdout)
    );
    let identity = super::super::support::git(
        None,
        &[
            "--git-dir",
            fixture.remote.to_str().unwrap(),
            "log",
            "-1",
            "--format=%an%x09%ae",
            "refs/heads/main",
        ],
    );
    assert_eq!(
        String::from_utf8_lossy(&identity.stdout).trim(),
        "Kool.ad/e Planner\tkoolade-migration@users.noreply.github.com"
    );
}

#[test]
fn remote_offline_keeps_the_operation_pending() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "offline change");
    super::super::support::git(
        Some(&fixture.first),
        &[
            "remote",
            "set-url",
            "origin",
            fixture.root.join("missing.git").to_str().unwrap(),
        ],
    );
    let state_root = fixture.state_root("offline");
    crate::core::planning_sync::runner::prepare_local_intent(
        &fixture.first,
        &state_root,
        "offline operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(status.state, PlanningSyncState::Offline);
    assert_eq!(status.pending_operations, 1);
}
