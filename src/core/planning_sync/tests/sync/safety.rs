mod commits;

use super::super::support::{Fixture, work_path};
use crate::core::managed_workspace::{CodeRepositoryRegistration, WorkspaceManager};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn missing_credentials_and_write_permission_have_distinct_statuses() {
    use crate::core::planning_sync::runner::transport_state;
    assert_eq!(
        transport_state("credentials are unavailable"),
        PlanningSyncState::AuthRequired
    );
    assert_eq!(
        transport_state("remote write permission is unavailable"),
        PlanningSyncState::ReadOnly
    );
    assert_eq!(
        transport_state(crate::core::planning_sync::git::classify_transport(
            "permission denied (publickey)."
        )),
        PlanningSyncState::AuthRequired
    );
}

#[test]
fn git_snapshot_reader_distinguishes_missing_files_from_invalid_commits() {
    let fixture = Fixture::new();
    let head = crate::core::planning_sync::git::head(&fixture.first).unwrap();
    assert_eq!(
        crate::core::planning_sync::git::read_path(
            &fixture.first,
            &head,
            "state/work/missing.json"
        )
        .unwrap(),
        None
    );
    assert!(
        crate::core::planning_sync::git::read_path(
            &fixture.first,
            "not-a-commit",
            &work_path(fixture.work_id)
        )
        .is_err()
    );
}

#[test]
fn git_snapshot_reader_rejects_oversized_remote_blobs_before_loading_them() {
    let fixture = Fixture::new();
    let record = work_path(fixture.work_id);
    let path = fixture.first.join(&record);
    fs::write(&path, vec![b'x'; 8 * 1024 * 1024 + 1]).unwrap();
    super::super::support::commit_all(&fixture.first, "oversized planning record");
    let head = crate::core::planning_sync::git::head(&fixture.first).unwrap();
    assert!(crate::core::planning_sync::git::read_path(&fixture.first, &head, &record).is_err());
}

#[test]
fn local_planning_mode_does_not_create_shared_operations() {
    let fixture = Fixture::new();
    super::super::support::git(Some(&fixture.first), &["remote", "remove", "origin"]);
    fixture.write_work(&fixture.first, fixture.work_id, "title", "local only");
    let state_root = fixture.state_root("local");
    let intent = prepare_local_intent(
        &fixture.first,
        &state_root,
        "local commit",
        &[work_path(fixture.work_id)],
    )
    .unwrap();
    assert!(intent.is_none());
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Local
    );
}

#[test]
fn manager_shared_sync_leaves_a_registered_code_checkout_untouched() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("manager-state");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager
        .join_remote(&fixture.remote.to_string_lossy())
        .unwrap();
    let planning_repo = workspace.planning_repo.clone();

    let code_repo = fixture.root.join("code-checkout");
    super::super::support::git(
        None,
        &[
            "-c",
            "init.defaultBranch=main",
            "init",
            code_repo.to_str().unwrap(),
        ],
    );
    fs::create_dir_all(code_repo.join("src")).unwrap();
    fs::write(code_repo.join("src/main.rs"), "fn main() {}\n").unwrap();
    super::super::support::commit_all(&code_repo, "initial code commit");
    super::super::support::git(Some(&code_repo), &["branch", "protected"]);
    let workspace = manager
        .register_code_repository(
            workspace.id,
            CodeRepositoryRegistration {
                id: "application".into(),
                role: "application code".into(),
                remote: None,
                checkout: code_repo.clone(),
            },
        )
        .unwrap();
    let head_before = String::from_utf8(
        super::super::support::git(Some(&code_repo), &["rev-parse", "HEAD"]).stdout,
    )
    .unwrap();
    let protected_before = String::from_utf8(
        super::super::support::git(Some(&code_repo), &["rev-parse", "refs/heads/protected"]).stdout,
    )
    .unwrap();

    fixture.write_work(
        &planning_repo,
        fixture.work_id,
        "title",
        "planning-only update",
    );
    prepare_local_intent(
        &planning_repo,
        &state_root,
        "sync planning update",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        manager.sync_planning(workspace.id).unwrap().state,
        PlanningSyncState::Synced
    );

    let head_after = String::from_utf8(
        super::super::support::git(Some(&code_repo), &["rev-parse", "HEAD"]).stdout,
    )
    .unwrap();
    let protected_after = String::from_utf8(
        super::super::support::git(Some(&code_repo), &["rev-parse", "refs/heads/protected"]).stdout,
    )
    .unwrap();
    let status = super::super::support::git(Some(&code_repo), &["status", "--porcelain=v1"]);
    assert_eq!(head_after, head_before);
    assert_eq!(protected_after, protected_before);
    assert!(
        status.stdout.is_empty(),
        "code index and worktree must stay clean"
    );
}

#[test]
fn manual_refresh_rejects_a_corrupt_remote_record_without_overwriting_local_data() {
    let fixture = Fixture::new();
    fs::write(
        fixture.second.join(work_path(fixture.work_id)),
        "not valid planning JSON\n",
    )
    .unwrap();
    super::super::support::commit_all(&fixture.second, "corrupt one planning record");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let status =
        sync_with_state_root(&fixture.first, &fixture.state_root("corrupt-remote")).unwrap();

    assert_eq!(status.state, PlanningSyncState::IncompatibleSchema);
    assert_eq!(status.pending_operations, 0);
    let local: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(local["data"]["title"], "original");
}

#[test]
fn manual_refresh_rejects_corrupt_planning_item_content() {
    let fixture = Fixture::new();
    let uid = uuid::Uuid::new_v4();
    let path = format!("planning/items/{uid}.md");
    fs::create_dir_all(fixture.second.join("planning/items")).unwrap();
    fs::write(fixture.second.join(&path), "not a managed item record\n").unwrap();
    super::super::support::commit_all(&fixture.second, "corrupt one planning item");
    super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let status = sync_with_state_root(&fixture.first, &fixture.state_root("corrupt-item")).unwrap();

    assert_eq!(status.state, PlanningSyncState::IncompatibleSchema);
    assert_eq!(status.pending_operations, 0);
    assert!(!fixture.first.join(&path).exists());
}

#[test]
fn shared_mode_rejects_authoritative_monolithic_writes() {
    let fixture = Fixture::new();
    fs::write(fixture.first.join("state/work.json"), "[]\n").unwrap();
    let state_root = fixture.state_root("legacy-layout");
    let error = prepare_local_intent(
        &fixture.first,
        &state_root,
        "legacy write",
        &["state/work.json".into()],
    )
    .unwrap_err();
    assert!(error.to_string().contains("per-record"));
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(status.state, PlanningSyncState::IncompatibleSchema);
    assert_eq!(status.pending_operations, 0);
}

#[test]
fn manual_refresh_does_not_claim_synced_with_unjournaled_local_edits() {
    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "not journaled");
    let state_root = fixture.state_root("local-edits");
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(status.state, PlanningSyncState::Pending);
    assert_eq!(status.pending_operations, 0);
}
