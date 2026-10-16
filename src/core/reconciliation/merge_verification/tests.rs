use super::*;
use std::{path::Path, process::Command};

struct Repo(std::path::PathBuf);

impl Repo {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("koolade-merge-proof-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        run(&path, &["init", "-q"]);
        run(&path, &["config", "user.name", "Synthetic Test"]);
        run(
            &path,
            &["config", "user.email", "synthetic@example.invalid"],
        );
        std::fs::write(path.join("README.md"), "base\n").unwrap();
        run(&path, &["add", "."]);
        run(&path, &["commit", "-qm", "base"]);
        Self(path)
    }

    fn merged(&self) -> (String, String, String) {
        let base = output(&self.0, &["rev-parse", "HEAD"]);
        run(&self.0, &["checkout", "-qb", "feature"]);
        std::fs::write(self.0.join("feature.txt"), "feature\n").unwrap();
        run(&self.0, &["add", "."]);
        run(&self.0, &["commit", "-qm", "feature"]);
        let feature = output(&self.0, &["rev-parse", "HEAD"]);
        run(&self.0, &["checkout", "-q", "-B", "main", &base]);
        run(&self.0, &["merge", "--no-ff", "-qm", "merge", "feature"]);
        (base, feature, output(&self.0, &["rev-parse", "HEAD"]))
    }

    fn squash_merged(&self) -> (String, String, String) {
        let (base, feature, _) = self.merged();
        run(&self.0, &["checkout", "-q", "-B", "squashed", &base]);
        run(&self.0, &["cherry-pick", "--no-commit", &feature]);
        run(&self.0, &["commit", "-qm", "squash"]);
        (base, feature, output(&self.0, &["rev-parse", "HEAD"]))
    }

    fn squash_merged_after_main_advances(&self) -> (String, String, String) {
        let (base, feature, _) = self.merged();
        run(&self.0, &["checkout", "-q", "-B", "main", &base]);
        std::fs::write(self.0.join("main.txt"), "main advanced\n").unwrap();
        run(&self.0, &["add", "."]);
        run(&self.0, &["commit", "-qm", "advance main"]);
        run(&self.0, &["cherry-pick", "--no-commit", &feature]);
        run(&self.0, &["commit", "-qm", "squash after main advanced"]);
        (base, feature, output(&self.0, &["rev-parse", "HEAD"]))
    }

    fn squash_merged_after_main_advanced_before_pr_open(&self) -> (String, String, String) {
        let (base, feature, _) = self.merged();
        run(&self.0, &["checkout", "-q", "-B", "main", &base]);
        std::fs::write(self.0.join("main.txt"), "main advanced\n").unwrap();
        run(&self.0, &["add", "."]);
        run(&self.0, &["commit", "-qm", "advance main before PR"]);
        let provider_base = output(&self.0, &["rev-parse", "HEAD"]);
        run(&self.0, &["cherry-pick", "--no-commit", &feature]);
        run(&self.0, &["commit", "-qm", "squash PR"]);
        (
            provider_base,
            feature,
            output(&self.0, &["rev-parse", "HEAD"]),
        )
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(repo: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    );
}

fn output(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}

#[test]
// Production boundary: local Git objects/ancestry prove a provider SHA alone
// cannot establish current state. Uses a temporary local Git repo; low setup.
fn validates_real_merged_git_evidence() {
    let repo = Repo::new();
    let (base, feature, merge) = repo.merged();
    let proof = validate_merged_checkout(&repo.0, &base, &feature, &merge).unwrap();
    assert_eq!(proof.pull_request_head, feature);
    assert_eq!(proof.merge_commit, merge);
    assert_eq!(proof.checked_out_head, merge);
}

#[test]
// Production boundary: rejects provider-controlled or nonexistent object IDs.
// Uses a temporary local Git repo; low setup and no network service.
fn rejects_forged_or_missing_reported_sha() {
    let repo = Repo::new();
    let (base, feature, _) = repo.merged();
    assert!(validate_merged_checkout(&repo.0, &base, &feature, &"f".repeat(40)).is_err());
    assert!(validate_merged_checkout(&repo.0, &base, "not-a-sha", &feature).is_err());
}

#[test]
// Production boundary: separates PR inclusion from checked-out target ancestry.
// Uses a temporary local Git repo; low setup and no network service.
fn rejects_ancestry_mismatch_and_merge_not_in_checked_out_head() {
    let repo = Repo::new();
    let (base, feature, merge) = repo.merged();
    run(&repo.0, &["checkout", "--orphan", "unrelated"]);
    run(&repo.0, &["rm", "-rf", "."]);
    std::fs::write(repo.0.join("unrelated.txt"), "unrelated\n").unwrap();
    run(&repo.0, &["add", "."]);
    run(&repo.0, &["commit", "-qm", "unrelated"]);
    let unrelated = output(&repo.0, &["rev-parse", "HEAD"]);
    assert!(validate_merged_checkout(&repo.0, &base, &unrelated, &merge).is_err());
    run(&repo.0, &["checkout", "-q", "--detach", &feature]);
    assert!(validate_merged_checkout(&repo.0, &base, &feature, &merge).is_err());
}

#[test]
// Production boundary: accepts a squash only when patch identity proves the
// reviewed changes landed. Uses a temporary local Git repo; low setup.
fn validates_squash_merge_by_exact_patch_identity() {
    let repo = Repo::new();
    let (base, feature, merge) = repo.squash_merged();
    let proof = validate_merged_checkout(&repo.0, &base, &feature, &merge).unwrap();
    assert_eq!(proof.pull_request_head, feature);
    assert_eq!(proof.merge_commit, merge);
}

#[test]
// Production boundary: a squash remains verifiable after the target branch
// advances between PR creation and merge. Local Git fixture, low setup cost.
fn validates_squash_when_main_advances_after_pr_creation() {
    let repo = Repo::new();
    let (provider_base, feature, merge) = repo.squash_merged_after_main_advances();
    validate_merged_checkout(&repo.0, &provider_base, &feature, &merge).unwrap();
}

#[test]
// Production boundary: a PR may target an advanced base while its reviewed
// head still shares an older merge-base. Local Git fixture; low setup cost.
fn validates_squash_when_main_advances_before_pr_creation() {
    let repo = Repo::new();
    let (provider_base, feature, merge) = repo.squash_merged_after_main_advanced_before_pr_open();
    validate_merged_checkout(&repo.0, &provider_base, &feature, &merge).unwrap();
}

#[test]
// Production boundary: GitHub's stable pull ref supplies the reviewed head
// after its source branch is deleted. Uses a local bare origin, no network.
fn fetches_exact_reviewed_head_after_source_branch_deletion() {
    let repo = Repo::new();
    let (_, feature, _) = repo.merged();
    run(&repo.0, &["init", "--bare", "-q", "origin.git"]);
    run(&repo.0, &["remote", "add", "origin", "origin.git"]);
    run(
        &repo.0,
        &["push", "-q", "origin", "feature:refs/pull/17/head"],
    );
    run(&repo.0, &["branch", "-D", "feature"]);
    fetch_pull_request_head(&repo.0, 17, &feature).unwrap();
    assert!(fetch_pull_request_head(&repo.0, 17, &"a".repeat(40)).is_err());
}

#[test]
// Production boundary: a valid PR number cannot redirect object fetches to a
// different origin or repository identity. Pure URL/identity check; no network.
fn rejects_pr_or_origin_repository_mismatch() {
    assert!(
        validate_pr_repository_binding(
            "github.com/example/project",
            "https://github.com/example/project/pull/17",
            17,
            "https://github.com/example/project.git",
            "git@github.com:example/project.git",
        )
        .is_ok()
    );
    assert!(
        validate_pr_repository_binding(
            "github.com/example/project",
            "https://github.com/attacker/repository/pull/17",
            17,
            "https://github.com/example/project.git",
            "git@github.com:example/project.git",
        )
        .is_err()
    );
    assert!(
        validate_pr_repository_binding(
            "github.com/example/project",
            "https://github.com/example/project/pull/17",
            17,
            "https://github.com/example/project.git",
            "git@github.com:attacker/repository.git",
        )
        .is_err()
    );
    assert!(
        validate_pr_repository_binding(
            "root",
            "https://github.com/example/project/pull/17",
            17,
            "https://github.com/attacker/repository.git",
            "git@github.com:example/project.git",
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
// Production boundary: fetch timeout owns the full Unix process group, not
// only Git's parent. Fake helper spawns a long-lived child; setup is local/cheap.
fn fetch_timeout_kills_a_spawned_remote_helper_descendant() {
    let root = std::env::temp_dir().join(format!(
        "koolade-fetch-tree-{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let pid_file = root.join("child.pid");
    let script = root.join("fake-helper.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 60 &\necho $! > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut command = std::process::Command::new("sh");
    command.arg(&script);
    let error = run_bounded_process(command, std::time::Duration::from_millis(200)).unwrap_err();
    assert!(error.to_string().contains("timed out"));
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let mut still_running = true;
    for _ in 0..40 {
        if unsafe { kill(pid, 0) } != 0 {
            still_running = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        !still_running,
        "timed-out remote helper child is still alive"
    );
    let _ = std::fs::remove_dir_all(root);
}
