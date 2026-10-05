use super::super::support::*;
use super::super::*;
use std::path::Path;

pub(super) enum DisjointMergeState {
    NotApplicable,
    Clean,
    UnexpectedChanges,
}

pub(super) fn inspect_disjoint_merge(
    repo: &Path,
    runner: &Runner,
    plan: &Plan,
    dir: &Path,
    state: &Implementation,
    head: &str,
    merge_head: Option<&str>,
) -> anyhow::Result<DisjointMergeState> {
    let worktree = &state.worktree;
    if head != plan.remote_commit
        || merge_head != Some(plan.local_commit.as_str())
        || !unmerged_paths(runner, worktree)?.is_empty()
        || !change_sets_are_disjoint(repo, runner, plan)?
    {
        return Ok(DisjointMergeState::NotApplicable);
    }
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
    let ignored = runner.git(
        worktree,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let generated = generated::trusted(runner, state, dir)?;
    let unknown = |paths: &str| {
        paths
            .split('\0')
            .filter(|path| !path.is_empty())
            .any(|path| !generated.contains(path))
    };
    if staged_paths != expected_local
        || !unstaged.is_empty()
        || unknown(&unexpected_paths)
        || unknown(&ignored)
    {
        return Ok(DisjointMergeState::UnexpectedChanges);
    }
    verify_disjoint_changes_preserved(worktree, runner, plan)?;
    Ok(DisjointMergeState::Clean)
}

pub(super) fn report(already_in_base: bool) -> serde_json::Value {
    let criteria = CONTRACT
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|criterion| {
            let evidence = if criterion.starts_with("Local and fetched shared changes") {
                if already_in_base {
                    "Both pinned source commits are already ancestors of the recorded task base; no duplicate dependency merge was needed."
                } else {
                    "The source histories changed separate paths; Git merged them cleanly and each changed path matches its source commit."
                }
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
        "verification": ["git diff --cached --name-only"],
        "remaining": [],
        "human_choices": []
    })
}

pub(super) fn current_merge_head(
    runner: &Runner,
    worktree: &Path,
) -> anyhow::Result<Option<String>> {
    match runner.git(worktree, &["rev-parse", "--verify", "MERGE_HEAD"]) {
        Ok(commit) => Ok(Some(commit)),
        Err(_) => {
            runner.remaining()?;
            Ok(None)
        }
    }
}

pub(super) fn cache_is_safe(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<bool> {
    let generated = generated::trusted(runner, state, dir)?;
    for args in [
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
        vec![
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    ] {
        if runner
            .git(&state.worktree, &args)?
            .split('\0')
            .filter(|path| !path.is_empty())
            .any(|path| !generated.contains(path))
        {
            return Ok(false);
        }
    }
    Ok(true)
}
