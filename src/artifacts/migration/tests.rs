use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn repo(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "koolade-artifact-migration-{tag}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&path).unwrap();
    assert!(git(&path, &["init", "-q"]).status.success());
    git_ok(&path, &["config", "user.name", "Fixture"]);
    git_ok(&path, &["config", "user.email", "fixture@example.test"]);
    path
}

fn git(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap()
}

fn git_ok(repo: &Path, args: &[&str]) -> String {
    let output = git(repo, args);
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit_all(repo: &Path, message: &str) {
    git_ok(repo, &["add", "-A"]);
    git_ok(repo, &["commit", "-q", "-m", message]);
}

#[test]
fn linked_worktrees_keep_artifact_migration_state_isolated() {
    let parent = std::env::temp_dir().join(format!(
        "koolade_migration_worktrees_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let primary = parent.join("primary");
    let linked = parent.join("linked");
    fs::create_dir_all(&primary).unwrap();
    assert!(git(&primary, &["init", "-q"]).status.success());
    fs::write(primary.join("README.md"), "fixture\n").unwrap();
    commit_all(&primary, "fixture");
    git_ok(
        &primary,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );

    let common = common_dir(&primary).unwrap();
    assert_eq!(common, common_dir(&linked).unwrap());
    assert_ne!(
        super::pending_path(&common, &primary).unwrap(),
        super::pending_path(&common, &linked).unwrap()
    );
    assert_ne!(
        super::lock_path(&common, &primary).unwrap(),
        super::lock_path(&common, &linked).unwrap()
    );
    let _ = fs::remove_dir_all(parent);
}

mod canonical_state;
mod files;
mod identities;
mod ignored_progress;
mod implementation_merge;
mod status;
mod task_state;

fn scoped_pending_path(root: &Path) -> PathBuf {
    super::pending_path(&common_dir(root).unwrap(), root).unwrap()
}
