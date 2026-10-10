use std::{path::PathBuf, process::Command};

use crate::core::managed_workspace::{WorkspaceManager, WorkspaceManifest};

use super::support::{TestDirectory, git_ok};

#[test]
fn manifest_validation_ignores_ambient_git_dir_and_work_tree() {
    if std::env::var_os("KOOLADE_MANIFEST_ENV_CHILD").is_some() {
        let planning_repo = PathBuf::from(
            std::env::var_os("KOOLADE_MANIFEST_ENV_REPO").expect("fixture path is passed"),
        );
        WorkspaceManifest::read(&planning_repo).unwrap();
        return;
    }

    let temp = TestDirectory::new("manifest-environment");
    let manager = WorkspaceManager::new(temp.path.join("operator"));
    let workspace = manager.create("Manifest environment").unwrap();
    let unrelated = temp.path.join("unrelated");
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=other",
            "--",
            unrelated.to_str().unwrap(),
        ],
    );

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "core::managed_workspace::tests::manifest_environment::manifest_validation_ignores_ambient_git_dir_and_work_tree",
        ])
        .env("KOOLADE_MANIFEST_ENV_CHILD", "1")
        .env("KOOLADE_MANIFEST_ENV_REPO", &workspace.planning_repo)
        .env("GIT_DIR", unrelated.join(".git"))
        .env("GIT_WORK_TREE", &unrelated)
        .output()
        .expect("run environment-isolated test child");
    assert!(
        output.status.success(),
        "child test failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn managed_creation_ignores_ambient_git_repository_environment() {
    if let Some(root) = std::env::var_os("KOOLADE_CREATE_ENV_ROOT") {
        let manager = WorkspaceManager::new(PathBuf::from(root).join("operator"));
        let workspace = manager.create("Managed creation environment").unwrap();
        assert_eq!(
            WorkspaceManifest::read(&workspace.planning_repo)
                .unwrap()
                .workspace_id,
            workspace.id
        );
        assert!(
            git_ok(
                Some(&workspace.planning_repo),
                &["rev-parse", "--verify", "HEAD"]
            )
            .status
            .success()
        );
        assert!(
            String::from_utf8(
                git_ok(
                    Some(&workspace.planning_repo),
                    &["status", "--porcelain=v1"]
                )
                .stdout
            )
            .unwrap()
            .is_empty()
        );
        return;
    }

    let temp = TestDirectory::new("creation-environment");
    let unrelated = temp.path.join("unrelated");
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=other",
            "--",
            unrelated.to_str().unwrap(),
        ],
    );
    git_ok(
        Some(&unrelated),
        &["config", "user.name", "Unrelated fixture"],
    );
    git_ok(
        Some(&unrelated),
        &["config", "user.email", "unrelated@example.invalid"],
    );
    std::fs::write(unrelated.join("README.md"), "unrelated\n").unwrap();
    git_ok(Some(&unrelated), &["add", "README.md"]);
    git_ok(Some(&unrelated), &["commit", "-qm", "unrelated"]);
    let unrelated_head =
        String::from_utf8(git_ok(Some(&unrelated), &["rev-parse", "HEAD"]).stdout).unwrap();
    let unrelated_git = unrelated.join(".git");

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "core::managed_workspace::tests::manifest_environment::managed_creation_ignores_ambient_git_repository_environment",
        ])
        .env("KOOLADE_CREATE_ENV_ROOT", &temp.path)
        .env("GIT_DIR", &unrelated_git)
        .env("GIT_WORK_TREE", &unrelated)
        .env("GIT_INDEX_FILE", unrelated_git.join("index"))
        .output()
        .expect("run environment-isolated workspace creation child");
    assert!(
        output.status.success(),
        "child test failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(git_ok(Some(&unrelated), &["rev-parse", "HEAD"]).stdout).unwrap(),
        unrelated_head,
        "managed-workspace creation must not change the unrelated repository"
    );
}
