use super::super::support::*;
use super::super::*;
use std::path::Path;

pub(super) fn clean_disjoint_merge(
    repo: &Path,
    worktree: &Path,
    runner: &Runner,
    plan: &Plan,
    head: &str,
    merge_head: Option<&str>,
) -> anyhow::Result<bool> {
    if head != plan.remote_commit
        || merge_head != Some(plan.local_commit.as_str())
        || !unmerged_paths(runner, worktree)?.is_empty()
        || !change_sets_are_disjoint(repo, runner, plan)?
    {
        return Ok(false);
    }
    verify_disjoint_changes_preserved(worktree, runner, plan)?;
    let staged = runner.git(
        worktree,
        &["diff", "--cached", "--name-only", "--no-renames", "-z"],
    )?;
    let expected_local = changed_paths(worktree, runner, &plan.common_base, &plan.local_commit)?;
    let staged_paths = staged
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let unexpected_paths = runner.git(
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let unstaged = runner.git(worktree, &["diff", "--name-only", "--no-renames", "-z"])?;
    anyhow::ensure!(
        staged_paths == expected_local && unstaged.is_empty() && unexpected_paths.is_empty(),
        "The clean merge includes unexpected worktree or index changes; reconciliation needs review"
    );
    Ok(true)
}

pub(super) fn report() -> serde_json::Value {
    let criteria = CONTRACT
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|criterion| {
            let evidence = if criterion.starts_with("Local and fetched shared changes") {
                "The source histories changed separate paths; Git merged them cleanly and each changed path matches its source commit."
            } else {
                "The application will run every repository-required baseline check on the combined result before accepting it."
            };
            serde_json::json!({ "criterion": criterion, "evidence": evidence })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schemaVersion": 2,
        "status": "complete",
        "blocker_disposition": "none",
        "summary": "Disjoint local and shared changes were combined; required verification will run before the baseline is accepted.",
        "acceptance_criteria": criteria,
        "verification": ["git diff --cached --check"],
        "remaining": [],
        "human_choices": []
    })
}
