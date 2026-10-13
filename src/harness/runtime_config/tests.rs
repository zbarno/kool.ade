use super::{GRANT_FILE, Grant, paths_with_source};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    root: PathBuf,
    _home: crate::test_support::KooladeHomeGuard,
}
impl Fixture {
    fn new() -> (Self, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("koolade-runtime-grant-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let home_guard = crate::test_support::KooladeHomeGuard::set(&home);
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--quiet"]);
        fs::write(source.join(".gitignore"), ".env\n").unwrap();
        fs::create_dir_all(source.join("App")).unwrap();
        fs::write(
            source.join("App/.env"),
            "SYNTHETIC_CONFIG=approved-sentinel\n",
        )
        .unwrap();
        let task = home
            .join("projects/project-1234567890abcdef/task-repositories/repo/task-1234567890abcdef");
        fs::create_dir_all(task.parent().unwrap()).unwrap();
        git(
            &source,
            &[
                "clone",
                "--quiet",
                source.to_str().unwrap(),
                task.to_str().unwrap(),
            ],
        );
        fs::write(task.join(".gitignore"), ".env\n").unwrap();
        fs::create_dir_all(task.join("App")).unwrap();
        fs::write(
            task.join("App/.env"),
            "SYNTHETIC_CONFIG=approved-sentinel\n",
        )
        .unwrap();
        let public_identity = "https://example.invalid/koolade/runtime-config.git";
        git(&source, &["remote", "add", "origin", public_identity]);
        git(&task, &["remote", "set-url", "origin", public_identity]);
        (
            Self {
                root,
                _home: home_guard,
            },
            source,
            task,
        )
    }
    fn grant(source: &Path, files: Vec<String>) {
        let common = source.join(".git");
        let dir = common.join("koolade");
        fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let record = common.join(GRANT_FILE);
        fs::write(
            &record,
            serde_json::to_vec(&Grant {
                schema_version: 1,
                source_root: source.canonicalize().unwrap(),
                files,
            })
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(record, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicit_runtime_configuration_grant_is_limited_to_ignored_untracked_env_files() {
    let (_fixture, source, task) = Fixture::new();
    Fixture::grant(&source, vec!["App/.env".into()]);
    assert_eq!(
        paths_with_source(&task, Some(&source)).unwrap(),
        ["App/.env".to_owned()].into()
    );
}

#[test]
fn runtime_configuration_rejects_traversal_duplicate_and_non_env_paths() {
    let (_fixture, source, task) = Fixture::new();
    for relative in [
        "../App/.env",
        "/tmp/.env",
        "App/config.json",
        "App/.env/../.env",
    ] {
        Fixture::grant(&source, vec![relative.into()]);
        assert!(
            paths_with_source(&task, Some(&source)).is_err(),
            "{relative}"
        );
    }
    Fixture::grant(&source, vec!["App/.env".into(), "App/.env".into()]);
    assert!(paths_with_source(&task, Some(&source)).is_err());
}
