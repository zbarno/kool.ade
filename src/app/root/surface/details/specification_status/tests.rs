use super::checkout_matches_current_evidence;
use crate::core::lifecycle::CurrentImplementationEvidenceV1;
use std::{path::Path, process::Command};

fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .expect("git available for local fixture");
    assert!(output.status.success(), "git fixture command failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn evidence(head: String) -> CurrentImplementationEvidenceV1 {
    CurrentImplementationEvidenceV1 {
        schema_version: 1,
        uid: "00000000-0000-4000-8000-000000000001".into(),
        feature_uid: "00000000-0000-4000-8000-000000000002".into(),
        task_uid: "00000000-0000-4000-8000-000000000003".into(),
        work_item_uid: "00000000-0000-4000-8000-000000000004".into(),
        repository_id: "root".into(),
        base_commit_sha: head.clone(),
        pull_request_head_sha: head.clone(),
        merge_commit_sha: head.clone(),
        checked_out_head_sha: head,
        specification_revision: 1,
        specification_hash: "a".repeat(64),
        revision: 1,
        recorded_at_ms: 1,
    }
}

#[test]
// Production boundary: the status loader's Git HEAD/worktree verification; a
// value-only unit test cannot catch a stale persisted projection after files
// change. Uses a synthetic local Git repository; no remote or service needed.
fn current_implementation_is_stale_after_head_or_worktree_changes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.name", "Fixture"]);
    git(repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("source.rs"), "fn value() -> u8 { 1 }\n").unwrap();
    git(repo, &["add", "source.rs"]);
    git(repo, &["commit", "-qm", "initial"]);
    let first = git(repo, &["rev-parse", "HEAD"]);
    assert!(checkout_matches_current_evidence(
        repo,
        &evidence(first.clone())
    ));

    std::fs::write(repo.join("source.rs"), "fn value() -> u8 { 2 }\n").unwrap();
    assert!(!checkout_matches_current_evidence(
        repo,
        &evidence(first.clone())
    ));
    git(repo, &["add", "source.rs"]);
    git(repo, &["commit", "-qm", "changed implementation"]);
    assert!(!checkout_matches_current_evidence(repo, &evidence(first)));
}
