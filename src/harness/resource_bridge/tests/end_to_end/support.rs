use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(super) struct TaskHome {
    _environment: crate::test_support::ProcessEnvironmentGuard,
}

pub(super) fn task_home(path: &Path) -> TaskHome {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", path);
    TaskHome {
        _environment: environment,
    }
}

pub(super) fn task_home_with_path(path: &Path, prepend_path: &Path) -> TaskHome {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", path);
    let mut paths = vec![prepend_path.to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    environment.set(
        "PATH",
        std::env::join_paths(paths).expect("test PATH entries should be valid"),
    );
    TaskHome {
        _environment: environment,
    }
}

pub(super) fn task_home_with_host_secret_probe(path: &Path) -> TaskHome {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", path);
    environment.set("KOOLADE_E2E_HOST_SECRET", "synthetic-test-sentinel");
    TaskHome {
        _environment: environment,
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
