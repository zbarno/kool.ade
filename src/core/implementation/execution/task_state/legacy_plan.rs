use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

#[allow(clippy::too_many_arguments)]
pub(super) fn attach_clone_identity(
    planning_root: &Path,
    repo: &Path,
    task_key: &str,
    text: &str,
    metadata: Option<&crate::artifacts::task_docs::TaskMetadata>,
    dir: &Path,
    plan: &initial_reconciliation::Plan,
    runner: &Runner,
) -> anyhow::Result<()> {
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_root)?;
    let repository_id = crate::core::implementation::task_repository_id(text, metadata, &manifest)?;
    let cache = RepositoryCache::open(repo, planning_root, &repository_id, runner)?;
    cache.validate_branch(&plan.base, runner)?;
    cache.import_local_branch(repo, &plan.base, &plan.local_commit, runner)?;
    let repo_path = repo
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 project repository path"))?;
    for commit in [&plan.local_commit, &plan.remote_commit, &plan.common_base] {
        if runner
            .git(
                &cache.path,
                &["cat-file", "-e", &format!("{commit}^{{commit}}")],
            )
            .is_err()
        {
            runner.git(
                &cache.path,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    repo_path,
                    commit,
                ],
            )?;
        }
        runner.git(
            &cache.path,
            &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        )?;
    }
    initial_reconciliation::support::pin_plan_commits(
        &cache.path,
        runner,
        task_key,
        &plan.local_commit,
        &plan.remote_commit,
    )?;
    initial_reconciliation::support::validate_pinned_commits(&cache.path, runner, task_key, plan)?;
    let clone_repository = initial_reconciliation::CloneRepositoryPlan {
        repository_id,
        repository_identity: cache.identity,
        source_ref: plan.base.clone(),
        destination_branch: metadata
            .and_then(|metadata| metadata.destination_branch.clone())
            .unwrap_or_else(|| plan.base.clone()),
    };
    initial_reconciliation::attach_clone_repository(dir, clone_repository)
}
