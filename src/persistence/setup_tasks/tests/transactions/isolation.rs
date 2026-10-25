use std::{fs, path::Path, process::Command};

use super::*;

/// Production boundary: separate workspaces on one machine must reconcile to
/// the same task IDs in one machine-local store without writing either project
/// Git tree. A model unit test cannot detect project-derived paths or writes.
/// Setup: two empty temporary Git repositories and one external state root;
/// requires Git and otherwise has negligible cost.
#[test]
fn two_workspace_scans_share_one_machine_store_without_project_writes() {
    let temp = tempfile::tempdir().unwrap();
    let projects = [temp.path().join("project-a"), temp.path().join("project-b")];
    let state_root = temp.path().join("operator-state");
    for project in &projects {
        fs::create_dir_all(project).unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(project)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(git_status(project).is_empty());
    }

    let path = transaction::path_for_state_root(&state_root);
    for index in 0..projects.len() {
        transaction::update_at(&path, |store| {
            store.latest_scan_generation += 1;
            let generation = store.latest_scan_generation;
            store.reconcile(
                &snapshot(
                    &[(TASK_ID, FindingStatus::NeedsAttention, &[])],
                    &["docker:rootless"],
                    generation + 100,
                ),
                generation,
            )?;
            Ok(((), true))
        })
        .unwrap();
        assert_eq!(
            transaction::path_for_state_root(&state_root),
            path,
            "workspace {index} should use the same machine store"
        );
    }
    let store = transaction::load_from(&path).unwrap().0;
    assert_eq!(store.tasks.len(), 1);
    assert_eq!(store.tasks[TASK_ID].id, TASK_ID);
    assert_eq!(store.latest_scan_generation, 2);
    assert!(path.starts_with(&state_root));
    assert!(projects.iter().all(|project| !path.starts_with(project)));
    assert!(path.is_file());
    for project in &projects {
        assert!(git_status(project).is_empty());
        assert!(!project.join(".koolade-packet").exists());
    }
}

fn git_status(project: &Path) -> Vec<u8> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(project)
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}
