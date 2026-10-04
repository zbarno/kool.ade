use std::{fs, path::PathBuf};

use crate::harness::pi_sandbox::{
    Sandbox,
    config::{locate_bwrap, locate_git},
};

use super::support::{TestTree, bwrap_available, create_worktree, git, run};

#[test]
fn worker_can_inspect_git_but_cannot_change_metadata_or_read_other_task_secrets() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let repository = tree.0.join("repository");
    let root = tree
        .0
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(&repository))
        .join("task worktree");
    fs::create_dir(&repository).unwrap();
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    git(&repository, &["init", "--quiet"]);
    fs::write(repository.join("tracked.txt"), "initial\n").unwrap();
    git(&repository, &["add", "tracked.txt"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Koolade test",
            "-c",
            "user.email=koolade-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    );
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "koolade-sandbox-test",
            root.to_str().unwrap(),
        ],
    );
    git(
        &root,
        &[
            "config",
            "--local",
            "remote.origin.url",
            "https://worker:secret@example.invalid/repository.git",
        ],
    );
    let other_root = root.parent().unwrap().join("other task");
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "koolade-sandbox-other",
            other_root.to_str().unwrap(),
        ],
    );
    let other_admin = git(
        &other_root,
        &["rev-parse", "--path-format=absolute", "--git-dir"],
    );
    let common = PathBuf::from(git(&root, &["rev-parse", "--git-common-dir"]));
    let private_log = common.join("koolade-harness").join("other-task.jsonl");
    fs::create_dir_all(private_log.parent().unwrap()).unwrap();
    fs::write(&private_log, "private other task activity").unwrap();
    fs::write(root.join("tracked.txt"), "changed\n").unwrap();

    let sandbox = Sandbox::new(&root).unwrap();
    let command = format!(
        "test \"$(git status --short)\" = ' M tracked.txt' && git diff --check && test -z \"$(git remote -v)\" && test ! -e '{}' && test ! -e '{}' && if printf 'bad\\n' > .git; then exit 46; fi && if git add tracked.txt 2>/dev/null; then exit 45; fi && git status --short",
        other_admin,
        private_log.display()
    );
    let output = run(&sandbox, "/bin/bash", &command);
    assert!(
        output.status.success(),
        "Git inspection or the read-only boundary failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "M tracked.txt"
    );
    assert_eq!(git(&root, &["status", "--short"]), "M tracked.txt");
}

#[test]
fn sandbox_rejects_main_checkout_and_unregistered_worktrees() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (repository, _registered) = create_worktree(&tree, "registered");
    assert!(Sandbox::new(&repository).is_err());
    let outside = tree.0.join("unregistered-worktree");
    let branch = format!("koolade-sandbox-outside-{}", uuid::Uuid::new_v4());
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            &branch,
            outside.to_str().unwrap(),
        ],
    );
    assert!(Sandbox::new(&outside).is_err());
}

#[cfg(unix)]
#[test]
fn executable_lookup_ignores_task_binaries_and_relative_path_entries() {
    use std::os::unix::fs::PermissionsExt;
    let _shield = crate::core::gitops::test_support::shield("sandbox-git-path");
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "git-path");
    let fake_git = root.join("git");
    let fake_bwrap = root.join("bwrap");
    fs::write(&fake_git, "#!/bin/sh\nexit 99\n").unwrap();
    fs::write(&fake_bwrap, "#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&fake_bwrap, fs::Permissions::from_mode(0o700)).unwrap();

    let previous = std::env::var_os("PATH");
    let previous_bwrap = std::env::var_os("KOOLADE_BWRAP_BIN");
    let system_bwrap_available = std::process::Command::new("bwrap")
        .arg("--version")
        .output()
        .is_ok();
    let mut entries = vec![PathBuf::from("."), root.clone()];
    if let Some(path) = previous.as_ref() {
        entries.extend(std::env::split_paths(path));
    }
    unsafe {
        std::env::set_var("PATH", std::env::join_paths(entries).unwrap());
        std::env::remove_var("KOOLADE_BWRAP_BIN");
    }
    let result = locate_git(&root);
    let bwrap_result = locate_bwrap(&root);
    unsafe {
        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        match previous_bwrap {
            Some(value) => std::env::set_var("KOOLADE_BWRAP_BIN", value),
            None => std::env::remove_var("KOOLADE_BWRAP_BIN"),
        }
    }

    let trusted_git = result.unwrap();
    assert!(!trusted_git.starts_with(&root));
    assert!(!trusted_git.starts_with(&tree.0));
    if system_bwrap_available {
        let trusted_bwrap = bwrap_result.unwrap();
        assert!(!trusted_bwrap.starts_with(&root));
        assert!(!trusted_bwrap.starts_with(&tree.0));
    }
}
