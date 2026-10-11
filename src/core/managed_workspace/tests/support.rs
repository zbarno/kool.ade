use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::core::managed_workspace::WorkspaceManager;

pub(super) struct TestDirectory {
    pub(super) path: PathBuf,
}

impl TestDirectory {
    pub(super) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "koolade-managed-workspace-{label}-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        fs::create_dir_all(&path).expect("create isolated test directory");
        Self { path }
    }

    pub(super) fn manager(&self, name: &str) -> WorkspaceManager {
        WorkspaceManager::new(self.path.join(name))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn git(cwd: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git command")
}

pub(super) fn git_ok(cwd: Option<&Path>, args: &[&str]) -> Output {
    let output = git(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub(super) fn code_checkout(
    root: &Path,
    id: &str,
    with_bare_origin: bool,
) -> (PathBuf, Option<String>) {
    let checkout = root.join("code").join(id);
    fs::create_dir_all(checkout.parent().expect("checkout parent")).unwrap();
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
            checkout.to_str().unwrap(),
        ],
    );
    if !with_bare_origin {
        return (checkout, None);
    }

    let remote = root.join("remotes").join(format!("{id}.git"));
    fs::create_dir_all(remote.parent().expect("remote parent")).unwrap();
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--bare",
            "--",
            remote.to_str().unwrap(),
        ],
    );
    let url = format!("ssh://fixture.invalid/team/{id}.git");
    git_ok(Some(&checkout), &["remote", "add", "origin", &url]);
    (checkout, Some(url))
}

pub(super) fn registration(
    id: &str,
    checkout: PathBuf,
    remote: Option<String>,
) -> crate::core::managed_workspace::CodeRepositoryRegistration {
    crate::core::managed_workspace::CodeRepositoryRegistration {
        id: id.into(),
        role: format!("{id} code repository"),
        remote,
        checkout,
    }
}
