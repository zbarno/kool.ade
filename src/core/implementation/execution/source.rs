use super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::implementation::repository_cache::RepositoryCache;
use anyhow::Context as _;

mod pinned;
mod refs;
use pinned::resolve_pinned_source;
pub(super) use refs::missing_saved_ref;

pub(super) struct TaskSource {
    pub(super) cache: RepositoryCache,
    pub(super) repository_id: String,
    pub(super) source_ref: String,
    pub(super) source_commit: String,
    pub(super) base_commit: String,
    pub(super) destination_branch: String,
}

pub(super) struct Request<'a> {
    pub(super) planning_store: &'a PlanningStore,
    pub(super) repo: &'a Path,
    pub(super) dir: &'a Path,
    pub(super) ticket: &'a str,
    pub(super) metadata: Option<&'a crate::artifacts::task_docs::TaskMetadata>,
    pub(super) runner: &'a Runner,
}

pub(super) fn resolve(request: Request<'_>) -> anyhow::Result<TaskSource> {
    let Request {
        planning_store,
        repo,
        dir,
        ticket,
        metadata,
        runner,
    } = request;
    let metadata = metadata
        .ok_or_else(|| anyhow::anyhow!("Executable task is missing its approved Git intent"))?;
    metadata.validate_for_execution()?;
    let repository_id = task_repository_id(Some(metadata))?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_store)?;
    let checkout = manifest.target(planning_store, repo, &repository_id)?;
    let cache = RepositoryCache::open(&checkout, planning_store, &repository_id, runner)?;
    let source_ref = metadata.source_branch.clone().ok_or_else(|| {
        anyhow::anyhow!("Select and approve a source branch before starting this task")
    })?;
    cache
        .validate_branch(&source_ref, runner)
        .with_context(|| {
            format!(
                "Selected source branch '{source_ref}' in code repository '{repository_id}' is invalid"
            )
        })?;

    let destination_branch = metadata.destination_branch.clone().ok_or_else(|| {
        anyhow::anyhow!("Select and approve a destination branch before starting this task")
    })?;
    cache
        .validate_branch(&destination_branch, runner)
        .with_context(|| {
            format!(
                "Selected destination branch '{destination_branch}' in code repository '{repository_id}' is invalid"
            )
        })?;
    validate_destination(
        &cache,
        &checkout,
        &repository_id,
        &destination_branch,
        runner,
    )?;

    let source_sha = metadata.source_sha.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Pin and approve a source commit before starting this task")
    })?;
    let (source_commit, base_commit) = resolve_pinned_source(pinned::Request {
        cache: &cache,
        repo: &checkout,
        source_ref: &source_ref,
        source_sha,
        dir,
        ticket,
        repository_id: &repository_id,
        destination_branch: &destination_branch,
        runner,
    })
    .map_err(|error| {
        anyhow::anyhow!(
            "Recorded source commit {source_sha} in code repository '{repository_id}': {error:#}"
        )
    })?;
    cache.pin_source(&source_commit, runner)?;
    Ok(TaskSource {
        cache,
        repository_id,
        source_ref,
        source_commit,
        base_commit,
        destination_branch,
    })
}

fn validate_destination(
    cache: &RepositoryCache,
    checkout: &Path,
    repository_id: &str,
    branch: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    if let Some(remote) = cache.fetch_url.as_deref() {
        let output = runner.git(
            &cache.path,
            &[
                "ls-remote",
                "--heads",
                remote,
                &format!("refs/heads/{branch}"),
            ],
        )?;
        anyhow::ensure!(
            !output.is_empty(),
            "Selected destination branch '{branch}' in code repository '{repository_id}' is no longer available on the repository remote"
        );
    } else {
        let reference = format!("refs/heads/{branch}^{{commit}}");
        anyhow::ensure!(
            runner
                .git(checkout, &["rev-parse", "--verify", &reference])
                .is_ok(),
            "Selected destination branch '{branch}' in code repository '{repository_id}' is unavailable in the local repository"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "source/tests.rs"]
mod tests;

pub(super) fn resume_from_plan(
    planning_store: &PlanningStore,
    repo: &Path,
    ticket: &str,
    metadata: Option<&crate::artifacts::task_docs::TaskMetadata>,
    plan: &crate::core::implementation::initial_reconciliation::Plan,
    runner: &Runner,
) -> anyhow::Result<TaskSource> {
    let metadata = metadata.ok_or_else(|| {
        anyhow::anyhow!("Resumed executable task is missing its approved Git intent")
    })?;
    metadata.validate_for_execution()?;
    let saved = plan
        .clone_repository
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Saved reconciliation plan has no task clone identity"))?;
    anyhow::ensure!(
        saved.source_ref == plan.base,
        "Saved task source reference does not match its reconciliation plan"
    );
    anyhow::ensure!(
        metadata.repository_id == saved.repository_id,
        "Task repository intent changed while its starting-point reconciliation was being prepared"
    );
    anyhow::ensure!(
        metadata.source_branch.as_deref() == Some(saved.source_ref.as_str())
            && metadata.destination_branch.as_deref() == Some(saved.destination_branch.as_str())
            && metadata.source_sha.as_deref() == Some(plan.local_commit.as_str()),
        "Task branch intent changed while its starting-point reconciliation was being prepared"
    );
    let cache_path =
        crate::core::implementation::repository_cache::cache_path(&saved.repository_identity)?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_store)?;
    let checkout = manifest.target(planning_store, repo, &saved.repository_id)?;
    let cache = RepositoryCache::open_at(
        &checkout,
        planning_store,
        &saved.repository_id,
        Some(&cache_path),
        runner,
    )?;
    anyhow::ensure!(
        cache.identity == saved.repository_identity,
        "Saved task repository identity does not match its application-owned cache"
    );
    crate::core::implementation::initial_reconciliation::support::validate_pinned_commits(
        &cache.path,
        runner,
        &crate::core::implementation::key_for_ticket(ticket),
        plan,
    )?;
    Ok(TaskSource {
        cache,
        repository_id: saved.repository_id.clone(),
        source_ref: saved.source_ref.clone(),
        source_commit: plan.local_commit.clone(),
        base_commit: plan.local_commit.clone(),
        destination_branch: saved.destination_branch.clone(),
    })
}
