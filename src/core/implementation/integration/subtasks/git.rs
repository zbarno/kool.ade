use super::super::super::*;
use super::receipt::IntegratedSubtask;
use std::{collections::BTreeSet, path::Path};

pub(super) fn squash_reviewed_child(
    parent_repo: &Path,
    child_repo: &str,
    task_uid: &str,
    display_id: &str,
    reviewed_head: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    ensure_clean(parent_repo, runner)?;
    runner.git(
        parent_repo,
        &["fetch", "--no-tags", child_repo, reviewed_head],
    )?;
    anyhow::ensure!(
        runner.git(parent_repo, &["rev-parse", "FETCH_HEAD"])? == reviewed_head,
        "Fetched subtask head differs from its independently reviewed commit"
    );
    runner.git(parent_repo, &["merge", "--squash", "FETCH_HEAD"])?;
    runner.git(parent_repo, &["add", "-A"])?;
    let message = format!(
        "Integrate reviewed subtask {display_id}\n\nKoolade-Subtask-UID: {task_uid}\nKoolade-Subtask-Head: {reviewed_head}"
    );
    runner.git(parent_repo, &["commit", "-m", &message])?;
    ensure_clean(parent_repo, runner)
}

pub(super) fn integrated_children(
    parent_repo: &Path,
    base_sha: &str,
    end_ref: &str,
    expected: &[IntegratedSubtask],
    runner: &Runner,
) -> anyhow::Result<BTreeSet<String>> {
    anyhow::ensure!(
        runner
            .git(
                parent_repo,
                &["merge-base", "--is-ancestor", base_sha, end_ref]
            )
            .is_ok(),
        "Parent subtask integration is not based on its approved source commit"
    );
    let commits = runner.git(
        parent_repo,
        &["rev-list", "--reverse", &format!("{base_sha}..{end_ref}")],
    )?;
    let mut found = BTreeSet::new();
    for commit in commits.lines().filter(|commit| !commit.is_empty()) {
        let message = runner.git(parent_repo, &["show", "-s", "--format=%B", commit])?;
        let uid = message
            .lines()
            .find_map(|line| line.strip_prefix("Koolade-Subtask-UID: "));
        let head = message
            .lines()
            .find_map(|line| line.strip_prefix("Koolade-Subtask-Head: "));
        let (Some(uid), Some(head)) = (uid, head) else {
            anyhow::bail!(
                "Parent clone contains a commit outside deterministic subtask integration"
            );
        };
        anyhow::ensure!(
            expected
                .iter()
                .any(|child| child.task_uid == uid && child.reviewed_head_sha == head)
                && found.insert(uid.to_owned()),
            "Parent clone contains an unknown, stale, or duplicate subtask integration commit"
        );
    }
    Ok(found)
}

pub(super) fn validate_receipt_binding(
    repo: &Path,
    receipt_base: &str,
    approved_base: &str,
    integration_head: &str,
    verified_head: &str,
    expected: &[IntegratedSubtask],
    runner: &Runner,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        receipt_base == approved_base,
        "Subtask integration receipt is bound to a different approved base"
    );
    anyhow::ensure!(
        runner
            .git(
                repo,
                &[
                    "merge-base",
                    "--is-ancestor",
                    integration_head,
                    verified_head,
                ],
            )
            .is_ok(),
        "Verified parent head does not contain its recorded subtask integration"
    );
    let integrated = integrated_children(repo, receipt_base, integration_head, expected, runner)?;
    anyhow::ensure!(
        integrated.len() == expected.len(),
        "Recorded integration head does not contain every reviewed subtask commit"
    );
    Ok(())
}

pub(super) fn ensure_clean(repo: &Path, runner: &Runner) -> anyhow::Result<()> {
    anyhow::ensure!(
        runner
            .git(repo, &["status", "--porcelain", "--untracked-files=all"])?
            .trim()
            .is_empty(),
        "Task clone contains uncommitted changes; preserving it for review"
    );
    Ok(())
}
