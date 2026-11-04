use super::*;
use std::{fs, process::Command};

#[test]
fn configured_origin_without_symbolic_default_never_pins_the_checked_out_branch() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--quiet", "-b", "main"]);
    git(repository.path(), &["config", "user.name", "Koolade Test"]);
    git(
        repository.path(),
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    fs::write(repository.path().join("base.txt"), "base\n").unwrap();
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "--quiet", "-m", "base"]);
    git(
        repository.path(),
        &["checkout", "--quiet", "-b", "feature/unpublished"],
    );
    git(
        repository.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/koolade.git",
        ],
    );

    let error = discover_ref(repository.path()).unwrap_err();
    assert!(error.to_string().contains("checked-out branch is not used"));
}

#[test]
fn local_only_repository_prefers_a_conventional_branch_over_checkout() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--quiet", "-b", "main"]);
    git(repository.path(), &["config", "user.name", "Koolade Test"]);
    git(
        repository.path(),
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    fs::write(repository.path().join("base.txt"), "base\n").unwrap();
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "--quiet", "-m", "base"]);
    git(
        repository.path(),
        &["checkout", "--quiet", "-b", "feature/unpublished"],
    );

    assert_eq!(discover_ref(repository.path()).unwrap(), "refs/heads/main");
}

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
