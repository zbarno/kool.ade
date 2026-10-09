use super::*;

pub(super) fn ensure_integration_repository(
    dir: &Path,
    state: &mut Implementation,
    integration_base: &str,
    commit_identity: &crate::core::implementation::repository_cache::GitCommitIdentity,
    runner: &Runner,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.task_repository_kind == TaskRepositoryKind::Clone,
        "Legacy integration repositories must migrate before use"
    );
    task_repository::validate_clone_path_at(state, &state.task_repository)?;
    let cache = RepositoryCache::from_saved_state(state, runner)?;
    if state.task_repository.exists() {
        RepositoryCache::verify_task_repository(&state.task_repository, runner)?;
        anyhow::ensure!(
            runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                == state.branch,
            "Saved integration clone is on another branch; preserved for review"
        );
        anyhow::ensure!(
            runner
                .git(
                    &state.task_repository,
                    &["merge-base", "--is-ancestor", integration_base, "HEAD"],
                )
                .is_ok(),
            "Saved integration clone no longer descends from its pinned destination; preserved"
        );
        crate::core::implementation::repository_cache::configure_clone(
            &state.task_repository,
            commit_identity,
            true,
            runner,
        )?;
        if !state.task_repository_ready {
            anyhow::ensure!(
                runner.git(&state.task_repository, &["rev-parse", "HEAD"])? == integration_base
                    && runner
                        .git(
                            &state.task_repository,
                            &["status", "--porcelain", "--untracked-files=all"]
                        )?
                        .is_empty(),
                "Uninitialized integration clone contains changes; preserved for review"
            );
            state.task_repository_ready = true;
            save(dir, state)?;
        }
        return Ok(());
    }
    anyhow::ensure!(
        !state.task_repository_ready,
        "Saved integration clone is missing; its path is preserved for review"
    );
    let source_ref = cache.pin_source(integration_base, runner)?;
    cache.create_clone(
        &source_ref,
        integration_base,
        &state.branch,
        &state.task_repository,
        commit_identity,
        runner,
    )?;
    RepositoryCache::verify_task_repository(&state.task_repository, runner)?;
    state.task_repository_ready = true;
    if !state.task_repositories.contains(&state.task_repository) {
        state.task_repositories.push(state.task_repository.clone());
    }
    save(dir, state)
}
