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
    support::validate_pinned_commits(
        repo,
        runner,
        &crate::core::implementation::key_for_ticket(&state.ticket),
        &plan,
    )?;
    requirements::refresh(repo, runner, &path, &mut plan)?;
    if let Some(verified) = plan.verified_commit.as_deref() {
        ensure_combines(repo, runner, &plan, verified)?;
        if state.base_commit != verified {
            state.base_commit = verified.into();
            save(dir, state)?;
        }
        return Ok(());
    }

    let _cache_lock = cache::acquire_lock(repo, &plan, runner)?;
    runner.update("Combining local and shared changes in an isolated worktree…");
    verification::prepare_worktree(repo, state, runner)?;
    validate_worktree(repo, state, runner)?;
    scope::ensure_pinned_path_scope(repo, &state.worktree, runner, &plan, dir, state, true)?;

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

    let head = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
    let local_in_head = is_ancestor(runner, &state.worktree, &plan.local_commit, &head)?;
    let remote_in_head = is_ancestor(runner, &state.worktree, &plan.remote_commit, &head)?;
    let mut merge_head = auto_verify::current_merge_head(runner, &state.worktree)?;
    let mut merge_in_progress = merge_head.is_some();
    if merge_in_progress {
        anyhow::ensure!(
            head == plan.remote_commit && merge_head.as_deref() == Some(plan.local_commit.as_str()),
            "The saved reconciliation worktree has unexpected merge parents. Its contents are preserved for review."
        );
    }

    let already_in_base = local_in_head && remote_in_head && !merge_in_progress;
    if already_in_base {
        scope::ensure_pinned_path_scope(repo, &state.worktree, runner, &plan, dir, state, true)?;
        let status = runner.git(&state.worktree, &["diff", "--name-only"])?;
        if !status.is_empty() {
            return Err(support::user_action(format!(
                "Both pinned prerequisite histories are already in the recorded base {}, but the task worktree has existing changes. They are preserved at {} for review; no duplicate dependency reconciliation was started.",
                head,
                state.worktree.display()
            )));
        }
    }

    if !(local_in_head && remote_in_head) && !merge_in_progress {
        let status = runner.git(&state.worktree, &["status", "--porcelain"])?;
        anyhow::ensure!(
            status.is_empty() && head == plan.remote_commit,
            "The saved reconciliation worktree has unexpected edits or a changed base. Its contents are preserved for review."
        );
        let result = runner.git(
            &state.worktree,
            &[
                "merge",
                "--no-ff",
                "--no-commit",
                "--no-edit",
                &plan.local_commit,
            ],
        );
        let unmerged = unmerged_paths(runner, &state.worktree)?;
        if let Err(error) = result {
            anyhow::ensure!(
                !unmerged.is_empty(),
                "Could not combine local and shared histories: {error}"
            );
        }
        merge_head = auto_verify::current_merge_head(runner, &state.worktree)?;
        merge_in_progress = merge_head.is_some();
        anyhow::ensure!(
            merge_in_progress,
            "Git did not leave a resumable merge in the isolated worktree"
        );
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
