use super::super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

pub(in crate::core::implementation) fn prepare_task_workspace(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    match state.task_repository_kind {
        TaskRepositoryKind::LegacyWorktree => prepare_worktree(repo, state, runner),
        TaskRepositoryKind::Clone => prepare_workspace(repo, dir, state, runner),
    }
}

pub(super) fn prepare_worktree(
    repo: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    anyhow::ensure!(
        state.task_repository_kind == TaskRepositoryKind::LegacyWorktree,
        "Legacy reconciliation requested a linked worktree for a task clone"
    );
    prepare_legacy_worktree(repo, state, runner)
}

pub(super) fn prepare_workspace(
    repository_cache: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    match state.task_repository_kind {
        TaskRepositoryKind::LegacyWorktree => {
            prepare_legacy_worktree(repository_cache, state, runner)
        }
        TaskRepositoryKind::Clone => prepare_clone(repository_cache, dir, state, runner),
    }
}

fn prepare_legacy_worktree(
    repo: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    runner.update("Preparing a legacy task workspace…");
    if state.task_repository.exists() {
        anyhow::ensure!(
            common(&state.task_repository)?.canonicalize()? == common(repo)?.canonicalize()?,
            "Existing legacy task workspace belongs to a different repository; no changes made"
        );
        anyhow::ensure!(
            runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                == state.branch,
            "Existing legacy task workspace is on another branch; no changes made"
        );
    } else {
        let path = state
            .task_repository
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 legacy task workspace path"))?;
        let branch_exists = runner
            .git(
                repo,
                &[
                    "show-ref",
                    "--verify",
                    &format!("refs/heads/{}", state.branch),
                ],
            )
            .is_ok();
        if branch_exists {
            runner.git_with_reflog_identity(repo, &["worktree", "add", path, &state.branch])?;
        } else {
            runner.git_with_reflog_identity(
                repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &state.branch,
                    path,
                    &state.base_commit,
                ],
            )?;
        }
    }
    let clean = runner
        .git(&state.task_repository, &["status", "--porcelain"])?
        .is_empty();
    let head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    Ok((clean, head))
}

fn prepare_clone(
    repository_cache_path: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    runner.update("Preparing the task repository…");
    task_repository::validate_clone_path(state)?;
    anyhow::ensure!(
        state.repository_cache.as_deref() == Some(repository_cache_path),
        "Task repository cache path changed; the saved clone is preserved"
    );
    let cache = RepositoryCache::from_saved_state(state, runner)?;
    let path = &state.task_repository;
    let saved_path_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(metadata) = saved_path_metadata {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Saved task repository is not a real directory; preserved for review"
        );
        anyhow::ensure!(
            path.canonicalize()?
                == path
                    .parent()
                    .unwrap()
                    .canonicalize()?
                    .join(path.file_name().unwrap()),
            "Saved task repository path changed; preserved for review"
        );
        RepositoryCache::verify_task_repository(path, runner)?;
        anyhow::ensure!(
            runner.git(path, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
            "Saved task repository is on another branch; preserved for review"
        );
        let commit_identity =
            crate::core::implementation::repository_cache::read_task_git_identity(dir)?;
        crate::core::implementation::repository_cache::configure_clone(
            path,
            &commit_identity,
            true,
            runner,
        )?;
        let head = runner.git(path, &["rev-parse", "HEAD"])?;
        if !state.task_repository_ready {
            anyhow::ensure!(
                head == state.base_commit
                    && runner.git(path, &["status", "--porcelain"])?.is_empty(),
                "An uninitialized task repository contains changes; it is preserved for review"
            );
            state.task_repository_ready = true;
            save(dir, state)?;
        } else {
            let pending_plan = crate::core::implementation::initial_reconciliation::load_plan(dir)?;
            let is_pinned_reconciliation_head = pending_plan.as_ref().is_some_and(|plan| {
                plan.verified_commit.is_none()
                    && plan.clone_repository.is_some()
                    && state.source_commit.as_deref() == Some(plan.local_commit.as_str())
                    && state.base_commit == plan.local_commit
                    && (head == plan.local_commit || head == plan.remote_commit)
            });
            anyhow::ensure!(
                is_pinned_reconciliation_head
                    || runner
                        .git(
                            path,
                            &["merge-base", "--is-ancestor", &state.base_commit, &head]
                        )
                        .is_ok(),
                "Saved task repository no longer descends from its pinned base; preserved for review"
            );
        }
        let clean = runner
            .git(path, &["status", "--porcelain", "--untracked-files=all"])?
            .is_empty();
        return Ok((clean, head));
    }

    anyhow::ensure!(
        !state.task_repository_ready,
        "Saved task repository is missing. Its path is preserved in task details; no fresh clone was created"
    );
    let source_commit = state
        .source_commit
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved task source commit is missing"))?;
    let commit_identity =
        crate::core::implementation::repository_cache::read_task_git_identity(dir)?;
    let source_branch = cache.pin_source(source_commit, runner)?;
    cache.create_clone(
        &source_branch,
        source_commit,
        &state.branch,
        path,
        &commit_identity,
        runner,
    )?;
    RepositoryCache::verify_task_repository(path, runner)?;
    let head = runner.git(path, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        head == source_commit,
        "Task clone did not start from its pinned source commit"
    );
    state.task_repository_ready = true;
    if !state.task_repositories.contains(path) {
        state.task_repositories.push(path.clone());
    }
    save(dir, state)?;
    Ok((true, head))
}
