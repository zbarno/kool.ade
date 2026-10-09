use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, MutexGuard},
};

static HOME_LOCK: Mutex<()> = Mutex::new(());

pub(super) struct TaskHome {
    previous: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

pub(super) fn task_home(path: &Path) -> TaskHome {
    let lock = HOME_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var_os("KOOLADE_HOME");
    unsafe { std::env::set_var("KOOLADE_HOME", path) };
    TaskHome {
        previous,
        _lock: lock,
    }
}

impl Drop for TaskHome {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(home) => std::env::set_var("KOOLADE_HOME", home),
                None => std::env::remove_var("KOOLADE_HOME"),
            }
        }
    }
}

pub(super) fn create_task_clone(repository: &Path, home: &Path, name: &str) -> PathBuf {
    let task_key = format!(
        "{}-{}",
        name,
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    );
    let path = home
        .join("projects")
        .join("synthetic-project")
        .join("task-repositories")
        .join(crate::persistence::project_slug(repository))
        .join(task_key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    run_git(
        repository,
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            repository.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
    run_git(&path, &["config", "user.name", "Synthetic Test"]);
    run_git(&path, &["config", "user.email", "test@example.invalid"]);
    run_git(
        &path,
        &["switch", "--quiet", "-c", &format!("koolade/{name}")],
    );
    path
}

pub(super) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

pub(super) fn initialize_project(root: &Path) {
    run_git(root, &["init", "--quiet"]);
}

pub(super) fn commit(root: &Path) {
    run_git(root, &["add", "--all"]);
    run_git(
        root,
        &[
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "initialize dependency e2e fixture",
        ],
    );
}

pub(super) fn with_npm_cache_index_staging(command: &str) -> String {
    let helper =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/harness/pi_sandbox/dependency_retry.mjs");
    let module = url::Url::from_file_path(helper).unwrap().to_string();
    let script = format!(
        "import {{ withPreparedNpmCacheIndex }} from {module:?}; process.stdout.write(withPreparedNpmCacheIndex({command:?}));"
    );
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", &script])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not prepare the npm offline command: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

pub(super) fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn npm_available() -> bool {
    Command::new("npm")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(super) fn bwrap_available() -> bool {
    Command::new("bwrap")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}
