use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::harness::pi_sandbox::Sandbox;

static HOME_LOCK: Mutex<()> = Mutex::new(());

pub(super) struct TestTree(
    pub(super) PathBuf,
    Option<OsString>,
    MutexGuard<'static, ()>,
);

impl TestTree {
    pub(super) fn new() -> Self {
        let lock = HOME_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "koolade-sandbox-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        let previous_home = std::env::var_os("KOOLADE_HOME");
        unsafe { std::env::set_var("KOOLADE_HOME", path.join("koolade-home")) };
        Self(path, previous_home, lock)
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = &self.2;
        unsafe {
            match self.1.take() {
                Some(home) => std::env::set_var("KOOLADE_HOME", home),
                None => std::env::remove_var("KOOLADE_HOME"),
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn git(directory: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command.stdin(std::process::Stdio::null());
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

pub(super) fn create_task_clone(tree: &TestTree, name: &str) -> (PathBuf, PathBuf) {
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
    let slug = name
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let task_key = format!(
        "{}-{}",
        if slug.is_empty() { "task" } else { &slug },
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    );
    let project_id = format!(
        "sandbox-project-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    );
    let repository_id = crate::persistence::project_slug(&repository);
    let root = crate::persistence::state_root()
        .join("projects")
        .join(project_id)
        .join("task-repositories")
        .join(repository_id)
        .join(task_key);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let branch = format!("koolade-sandbox-test-{}", uuid::Uuid::new_v4());
    git(
        &tree.0,
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            repository.to_str().unwrap(),
            root.to_str().unwrap(),
        ],
    );
    git(&root, &["config", "user.name", "Koolade test"]);
    git(
        &root,
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    git(&root, &["switch", "--quiet", "-c", &branch]);
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
