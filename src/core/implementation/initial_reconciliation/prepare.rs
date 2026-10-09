mod auto_verify;
mod external_blocker;
mod reconcile;
mod recovery;
mod requirements;
mod scope;
mod whitespace;

use super::support::*;
use super::*;

pub fn prepare(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    user_context: Option<&str>,
    accrual: Option<&crate::core::time_accrual::AgentSpan>,
) -> anyhow::Result<()> {
    let path = dir.join(PLAN_FILE);
    if !path.exists() {
        return Ok(());
    }
    let mut plan = read_plan(&path)?;
    support::validate_pinned_commits(repo, runner, &task_repository::allocation_key(state), &plan)?;
    requirements::refresh(repo, runner, &path, &mut plan)?;
    if let Some(verified) = plan.verified_commit.as_deref() {
        anyhow::ensure!(
            state.task_repository_kind == TaskRepositoryKind::Clone,
            "Legacy reconciliation repositories must migrate before use"
        );
        let (clean, head) = verification::prepare_task_workspace(repo, dir, state, runner)?;
        import_pinned_commits(repo, state, &plan, runner)?;
        if !is_ancestor(runner, &state.task_repository, verified, &head)? {
            anyhow::ensure!(
                clean && is_ancestor(runner, &state.task_repository, &head, verified)?,
                "Saved verified baseline is not a safe fast-forward from the task clone; its contents are preserved"
            );
            runner.git(&state.task_repository, &["reset", "--hard", verified])?;
        }
        let history = &state.task_repository;
        ensure_combines(history, runner, &plan, verified)?;
        if state.base_commit != verified {
            state.base_commit = verified.into();
            save(dir, state)?;
        }
        return Ok(());
    }

    let _cache_lock = cache::acquire_lock(repo, &plan, runner)?;
    runner.update("Combining local and shared changes in an isolated task repository…");
    verification::prepare_task_workspace(repo, dir, state, runner)?;
    if state.task_repository_kind == TaskRepositoryKind::Clone {
        import_pinned_commits(repo, state, &plan, runner)?;
    }
    validate_task_repository(state, runner)?;
    scope::ensure_pinned_path_scope(
        repo,
        &state.task_repository,
        runner,
        &plan,
        dir,
        state,
        true,
    )?;

    if auto_verify::cache_is_safe(runner, state, dir)?
        && let Some(cached) = cache::load(repo, &plan, runner)?
        && cache::adopt(repo, state, &plan, &cached, runner)?
    {
        plan.verified_commit = Some(cached.verified_commit.clone());
        plan.verification = cached.verification;
        write_plan(&path, &plan)?;
        state.base_commit = cached.verified_commit;
        save(dir, state)?;
        runner
            .update("Reused the verified shared baseline; starting the requested implementation…");
        return Ok(());
    }

    let mut head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    let mut local_in_head = is_ancestor(runner, &state.task_repository, &plan.local_commit, &head)?;
    let mut remote_in_head =
        is_ancestor(runner, &state.task_repository, &plan.remote_commit, &head)?;
    let mut merge_head = auto_verify::current_merge_head(runner, &state.task_repository)?;
    let mut merge_in_progress = merge_head.is_some();
    if merge_in_progress {
        anyhow::ensure!(
            head == plan.remote_commit && merge_head.as_deref() == Some(plan.local_commit.as_str()),
            "The saved reconciliation repository has unexpected merge parents. Its contents are preserved for review."
        );
    }

    if !(local_in_head && remote_in_head) && !merge_in_progress {
        let status = runner.git(&state.task_repository, &["status", "--porcelain"])?;
        anyhow::ensure!(
            status.is_empty(),
            "The saved reconciliation repository has unexpected edits or a changed base. Its contents are preserved for review."
        );
        if head == plan.local_commit {
            anyhow::ensure!(
                state.task_repository_kind == TaskRepositoryKind::Clone
                    && plan.clone_repository.is_some()
                    && state.source_commit.as_deref() == Some(plan.local_commit.as_str()),
                "The saved reconciliation repository has an unexpected source base. Its contents are preserved for review."
            );
            runner.git(
                &state.task_repository,
                &["reset", "--hard", &plan.remote_commit],
            )?;
            head = plan.remote_commit.clone();
            local_in_head = is_ancestor(runner, &state.task_repository, &plan.local_commit, &head)?;
            remote_in_head =
                is_ancestor(runner, &state.task_repository, &plan.remote_commit, &head)?;
        }
        if !(local_in_head && remote_in_head) {
            anyhow::ensure!(
                head == plan.remote_commit,
                "The saved reconciliation repository has an unexpected base. Its contents are preserved for review."
            );
            let result = runner.git(
                &state.task_repository,
                &[
                    "merge",
                    "--no-ff",
                    "--no-commit",
                    "--no-edit",
                    &plan.local_commit,
                ],
            );
            let unmerged = unmerged_paths(runner, &state.task_repository)?;
            if let Err(error) = result {
                anyhow::ensure!(
                    !unmerged.is_empty(),
                    "Could not combine local and shared histories: {error}"
                );
            }
            merge_head = auto_verify::current_merge_head(runner, &state.task_repository)?;
            merge_in_progress = merge_head.is_some();
            anyhow::ensure!(
                merge_in_progress,
                "Git did not leave a resumable merge in the isolated task repository"
            );
        }
    }

    let already_in_base = local_in_head && remote_in_head && !merge_in_progress;
    if already_in_base {
        scope::ensure_pinned_path_scope(
            repo,
            &state.task_repository,
            runner,
            &plan,
            dir,
            state,
            true,
        )?;
        let status = runner.git(&state.task_repository, &["diff", "--name-only"])?;
        if !status.is_empty() {
            return Err(support::user_action(format!(
                "Both pinned prerequisite histories are already in the recorded base {}, but the task repository has existing changes. They are preserved at {} for review; no duplicate dependency reconciliation was started.",
                head,
                state.task_repository.display()
            )));
        }
    }

    let automatically_verified = match auto_verify::inspect_disjoint_merge(
        repo,
        runner,
        &plan,
        dir,
        state,
        &head,
        merge_head.as_deref(),
    )? {
        auto_verify::DisjointMergeState::Clean => true,
        auto_verify::DisjointMergeState::NotApplicable => already_in_base,
        auto_verify::DisjointMergeState::UnexpectedChanges => {
            let merge_head = merge_head.as_deref().unwrap_or_default();
            recovery::recover_unexpected_merge(
                repo,
                dir,
                state,
                runner,
                &plan,
                merge_head,
                state.detail.starts_with("Resume accepted;"),
            )?;
            match auto_verify::inspect_disjoint_merge(
                repo,
                runner,
                &plan,
                dir,
                state,
                &head,
                Some(merge_head),
            )? {
                auto_verify::DisjointMergeState::Clean => true,
                _ => {
                    return Err(support::user_action(format!(
                        "Unexpected changes remain after the one automatic reconciliation retry. Review the preserved snapshot at {}.",
                        dir.join(recovery::SNAPSHOT_FILE).display()
                    )));
                }
            }
        }
    };

    reconcile::run(
        reconcile::Context {
            repo,
            dir,
            path: &path,
            state,
            harness,
            runner,
            user_context,
            accrual,
            plan: &mut plan,
        },
        automatically_verified,
        already_in_base,
    )
}

fn import_pinned_commits(
    cache: &Path,
    state: &Implementation,
    plan: &Plan,
    runner: &Runner,
) -> anyhow::Result<()> {
    let cache_path = cache
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository cache path"))?;
    let task_key = task_repository::allocation_key(state);
    for (side, expected) in [
        ("local", plan.local_commit.as_str()),
        ("remote", plan.remote_commit.as_str()),
    ] {
        let source = format!("refs/koolade-reconciliations/{task_key}/{side}");
        let destination = format!("refs/koolade-reconciliation-inputs/{task_key}/{side}");
        runner.git(
            &state.task_repository,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                cache_path,
                &format!("+{source}:{destination}"),
            ],
        )?;
        anyhow::ensure!(
            runner.git(
                &state.task_repository,
                &["rev-parse", "--verify", &destination],
            )? == expected,
            "Task repository received a different pinned {side} commit"
        );
    }
    runner.git(
        &state.task_repository,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", plan.common_base),
        ],
    )?;
    Ok(())
}
