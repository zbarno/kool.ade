use super::super::support::{Fixture, work_path};
use crate::core::managed_workspace::WorkspaceManager;
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn shared_sync_never_imports_planning_from_a_source_branch() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("branch-isolation");
    let manager = WorkspaceManager::new(&state_root);
    let workspace = manager
        .join_remote(&fixture.remote.to_string_lossy())
        .unwrap();
    let planning_repo = workspace.planning_repo.clone();
    assert_eq!(
        manager.sync_planning(workspace.id).unwrap().state,
        PlanningSyncState::Synced
    );
    fixture.write_work(
        &planning_repo,
        fixture.work_id,
        "title",
        "canonical pending edit",
    );
    prepare_local_intent(
        &planning_repo,
        &state_root,
        "canonical pending edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    super::super::support::git(
        Some(&planning_repo),
        &["switch", "--quiet", "-c", "feature/source-planning"],
    );
    let feature_only_id = uuid::Uuid::new_v4();
    fs::create_dir_all(planning_repo.join("state/work")).unwrap();
    fs::write(
        planning_repo.join(work_path(feature_only_id)),
        super::super::support::record(feature_only_id, "feature-only", "private branch data", 1),
    )
    .unwrap();
    super::super::support::commit_all(&planning_repo, "source branch planning changes");
    let remote_head_before = String::from_utf8(
        super::super::support::git(
            None,
            &[
                "--git-dir",
                fixture.remote.to_str().unwrap(),
                "rev-parse",
                "refs/heads/main",
            ],
        )
        .stdout,
    )
    .unwrap();

    let status = sync_with_state_root(&planning_repo, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending);
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("not on refs/heads/main")
    );
    assert!(
        format!("{}", manager.open(workspace.id).unwrap_err())
            .contains("must be on canonical branch main")
    );
    let remote_head_after = String::from_utf8(
        super::super::support::git(
            None,
            &[
                "--git-dir",
                fixture.remote.to_str().unwrap(),
                "rev-parse",
                "refs/heads/main",
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(remote_head_after, remote_head_before);
    assert_ne!(
        remote_head_after,
        String::from_utf8(
            super::super::support::git(Some(&planning_repo), &["rev-parse", "HEAD"]).stdout,
        )
        .unwrap()
    );
}
