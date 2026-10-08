use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::harness::pi_sandbox::Sandbox;

pub(super) struct TestTree(pub(super) PathBuf);

impl TestTree {
    pub(super) fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "koolade-sandbox-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn git(directory: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    if args.starts_with(&["worktree", "add"]) {
        command.args([
            "-c",
            "user.name=Koolade test",
            "-c",
            "user.email=koolade-test@example.invalid",
        ]);
    }
    let output = command.args(args).current_dir(directory).output().unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().into()
}

pub(super) fn bwrap_available() -> bool {
    Command::new("bwrap").arg("--version").output().is_ok()
}

pub(super) fn create_worktree(tree: &TestTree, name: &str) -> (PathBuf, PathBuf) {
    let repository = tree.0.join("repository");
    fs::create_dir(&repository).unwrap();
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
    let root = tree
        .0
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(&repository))
        .join(name);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let branch = format!("koolade-sandbox-test-{}", uuid::Uuid::new_v4());
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            &branch,
            root.to_str().unwrap(),
        ],
    );
    (repository, root)
}

pub(super) fn run(sandbox: &Sandbox, shell: &str, command: &str) -> std::process::Output {
    Command::new(&sandbox.bwrap)
        .args(sandbox.command_args(shell, command))
        .current_dir(&sandbox.root)
        .env_clear()
        .env("AWS_ACCESS_KEY_ID", "koolade-test-secret-sentinel")
        .output()
        .unwrap()
}
