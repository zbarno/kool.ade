use super::*;

/// A completed task already has independent evidence for an immutable head.
/// Preserve that identity when its reviewed base is still current instead of
/// manufacturing an equivalent squash commit and discarding all gate bindings.
pub(in crate::core::implementation) fn prepare_reviewed_for_pull_request(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    runner.remaining()?;
    task_repository::validate_clone_path(state)?;
    let cache = RepositoryCache::from_saved_state(state, runner)?;
    cache.validate_task_remote(&state.task_repository, runner)?;
    let current_base = cache.refresh_branch(&state.base, runner)?;
    anyhow::ensure!(
        initial_reconciliation::support::generated::clean(runner, state, dir)?,
        "Reviewed task clone contains unverified changes; preserved for review"
    );
    let actual_head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        state.verified_head.as_deref() == Some(actual_head.as_str()),
        "Reviewed task clone head changed before PR publication"
    );
    if current_base == state.base_commit {
        runner.update("Reviewed destination base is current; preserving the independently reviewed commit for publication…");
        return Ok(());
    }
    // Existing integration preserves user work and verifies the new candidate.
    // The publication lifecycle subsequently rejects its changed head until
    // independent review, QA and spec evidence are renewed.
    prepare_for_pull_request(repo, dir, state, harness, runner, policy)
}
