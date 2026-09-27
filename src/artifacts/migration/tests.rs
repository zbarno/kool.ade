use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};

fn repo(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "packet-artifact-migration-{tag}-{}-{}",
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

mod canonical_state;
mod files;
mod identities;
mod implementation_merge;
mod status;
mod task_state;
