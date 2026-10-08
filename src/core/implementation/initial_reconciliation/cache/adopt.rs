use super::super::{Plan, support};
use super::{VerifiedBase, cache_key, cache_ref};
use crate::core::implementation::{
    Implementation, Runner, TaskRepositoryKind, state_paths::common, task_repository,
};

pub(in crate::core::implementation) fn adopt(
    repo: &std::path::Path,
    state: &Implementation,
    plan: &Plan,
    cached: &VerifiedBase,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let branch_ref = format!("refs/heads/{}", state.branch);
    if !state.task_repository.exists() {
        let current = match runner.git(repo, &["rev-parse", "--verify", &branch_ref]) {
            Ok(commit) => Some(commit),
            Err(_) => {
                runner.remaining()?;
                None
            }
        };
        if let Some(current) = current {
            if current == plan.remote_commit {
                runner.git(
                    repo,
                    &["update-ref", &branch_ref, &cached.verified_commit, &current],
                )?;
            } else if current != cached.verified_commit {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    match state.task_repository_kind {
        TaskRepositoryKind::LegacyWorktree => anyhow::ensure!(
            common(&state.task_repository)?.canonicalize()? == common(repo)?.canonicalize()?
                && runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                    == state.branch,
            "Existing task repository identity changed; refusing to adopt a cached baseline"
        ),
        TaskRepositoryKind::Clone => {
            crate::core::implementation::task_repository::validate_clone_path(state)?;
            crate::core::implementation::repository_cache::RepositoryCache::verify_task_repository(
                &state.task_repository,
                runner,
            )?;
            anyhow::ensure!(
                runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                    == state.branch,
                "Existing task repository identity changed; refusing to adopt a cached baseline"
            );
        }
    }
    if state.task_repository_kind == TaskRepositoryKind::Clone {
        let cache_path = repo
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository cache path"))?;
        let task_key = task_repository::allocation_key(state);
        let destination = format!("refs/koolade-reconciliation-inputs/{task_key}/verified");
        let refspec = format!("+{}:{destination}", cache_ref(&cache_key(plan)));
        runner.git(
            &state.task_repository,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                cache_path,
                &refspec,
            ],
        )?;
        anyhow::ensure!(
            runner.git(
                &state.task_repository,
                &["rev-parse", "--verify", &destination],
            )? == cached.verified_commit,
            "Task repository received a different cached baseline"
        );
    }
    let head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    let merge_head = match runner.git(
        &state.task_repository,
        &["rev-parse", "--verify", "MERGE_HEAD"],
    ) {
        Ok(commit) => Some(commit),
        Err(_) => {
            runner.remaining()?;
            None
        }
    };
    if let Some(merge_head) = merge_head {
        if head != plan.remote_commit || merge_head != plan.local_commit {
            return Ok(false);
        }
        if !support::unmerged_paths(runner, &state.task_repository)?.is_empty()
            || !runner
                .git(&state.task_repository, &["diff", "--name-only"])?
                .is_empty()
            || !runner
                .git(
                    &state.task_repository,
                    &["ls-files", "--others", "--exclude-standard", "-z"],
                )?
                .is_empty()
            || !runner
                .git(
                    &state.task_repository,
                    &[
                        "ls-files",
                        "--others",
                        "--ignored",
                        "--exclude-standard",
                        "-z",
                    ],
                )?
                .is_empty()
        {
            return Ok(false);
        }
        let current_tree = runner.git(&state.task_repository, &["write-tree"])?;
        let verified_tree = runner.git(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{tree}}", cached.verified_commit),
            ],
        )?;
        if current_tree != verified_tree {
            return Ok(false);
        }
        runner.git(
            &state.task_repository,
            &["reset", "--hard", &cached.verified_commit],
        )?;
        return Ok(runner
            .git(&state.task_repository, &["status", "--porcelain"])?
            .is_empty());
    }

    if !runner
        .git(&state.task_repository, &["status", "--porcelain"])?
        .is_empty()
    {
        return Ok(false);
    }
    if head == cached.verified_commit {
        return Ok(true);
    }
    if state.task_repository_kind == TaskRepositoryKind::Clone
        && head == plan.local_commit
        && state.source_commit.as_deref() == Some(plan.local_commit.as_str())
        && state.base_commit == plan.local_commit
    {
        runner.git(
            &state.task_repository,
            &["reset", "--hard", &cached.verified_commit],
        )?;
        anyhow::ensure!(
            runner.git(&state.task_repository, &["rev-parse", "HEAD"])? == cached.verified_commit
                && runner
                    .git(&state.task_repository, &["status", "--porcelain"])?
                    .is_empty(),
            "Cached baseline reset left unexpected task repository changes"
        );
        return Ok(true);
    }
    if head != plan.remote_commit {
        return Ok(false);
    }
    runner.git(
        &state.task_repository,
        &["merge", "--ff-only", &cached.verified_commit],
    )?;
    anyhow::ensure!(
        runner.git(&state.task_repository, &["rev-parse", "HEAD"])? == cached.verified_commit,
        "Cached baseline fast-forward did not reach its verified commit"
    );
    anyhow::ensure!(
        runner
            .git(&state.task_repository, &["status", "--porcelain"])?
            .is_empty(),
        "Cached baseline fast-forward left task repository changes; preserving them for review"
    );
    let index_tree = runner.git(&state.task_repository, &["write-tree"])?;
    let commit_tree = runner.git(
        &state.task_repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{tree}}", cached.verified_commit),
        ],
    )?;
    anyhow::ensure!(
        index_tree == commit_tree,
        "Cached baseline fast-forward left an unexpected index; preserving it for review"
    );
    Ok(true)
}
