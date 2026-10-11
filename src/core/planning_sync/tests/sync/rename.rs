use super::super::support::Fixture;
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[test]
fn local_committed_rename_publishes_the_source_deletion() {
    let fixture = Fixture::new();
    let source = "planning/notes.md";
    let destination = "planning/renamed-notes.md";
    fs::create_dir_all(fixture.first.join("planning")).unwrap();
    fs::write(fixture.first.join(source), "shared note\n").unwrap();
    super::super::support::commit_all(&fixture.first, "add planning note");
    push_main(&fixture.first);

    super::super::support::git(Some(&fixture.first), &["mv", "--", source, destination]);
    super::super::support::commit_all(&fixture.first, "rename planning note");
    let state_root = fixture.state_root("local-rename");

    prepare_local_intent(&fixture.first, &state_root, "rename planning note", &[])
        .unwrap()
        .unwrap();
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    let confirmed = status.confirmed_commit.as_deref().unwrap();

    assert_eq!(status.state, PlanningSyncState::Synced);
    assert_eq!(
        crate::core::planning_sync::git::read_path(&fixture.first, confirmed, source).unwrap(),
        None
    );
    assert_eq!(
        crate::core::planning_sync::git::read_path(&fixture.first, confirmed, destination).unwrap(),
        Some(b"shared note\n".to_vec())
    );
}

#[test]
fn remote_rename_refresh_removes_the_old_local_path() {
    let fixture = Fixture::new();
    let source = "planning/notes.md";
    let destination = "planning/renamed-notes.md";
    fs::create_dir_all(fixture.second.join("planning")).unwrap();
    fs::write(fixture.second.join(source), "shared note\n").unwrap();
    super::super::support::commit_all(&fixture.second, "add planning note");
    push_main(&fixture.second);

    let state_root = fixture.state_root("remote-rename");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );
    super::super::support::git(Some(&fixture.second), &["mv", "--", source, destination]);
    super::super::support::commit_all(&fixture.second, "rename planning note");
    push_main(&fixture.second);

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Synced);
    assert!(!fixture.first.join(source).exists());
    assert_eq!(
        fs::read(fixture.first.join(destination)).unwrap(),
        b"shared note\n"
    );
}

fn push_main(repo: &std::path::Path) {
    super::super::support::git(Some(repo), &["push", "origin", "HEAD:refs/heads/main"]);
}
