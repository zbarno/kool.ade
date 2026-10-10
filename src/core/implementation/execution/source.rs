use super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::implementation::initial_reconciliation::CloneRepositoryPlan;
use crate::core::implementation::repository_cache::RepositoryCache;

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
    pub(super) text: &'a str,
    pub(super) metadata: Option<&'a crate::artifacts::task_docs::TaskMetadata>,
    pub(super) publication_mode: PublicationMode,
    pub(super) runner: &'a Runner,
}

pub(super) fn resolve(request: Request<'_>) -> anyhow::Result<TaskSource> {
    let Request {
        planning_store,
        repo,
        dir,
        ticket,
        text,
        metadata,
        publication_mode,
        runner,
    } = request;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_store)?;
    let repository_id = task_repository_id(text, metadata, &manifest)?;
    let cache = RepositoryCache::open(repo, planning_store, &repository_id, runner)?;
    let explicitly_selected = metadata.and_then(|metadata| metadata.source_branch.as_deref());
    let source_ref = if let Some(source) = explicitly_selected {
        source.to_owned()
    } else if publication_mode == PublicationMode::AutoPublish {
        publication::default_branch(repo, runner)?
    } else {
        runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?
    };
    anyhow::ensure!(
        !source_ref.is_empty(),
        "Select a source branch before starting this task"
    );
    cache.validate_branch(&source_ref, runner)?;

    let destination_branch = metadata
        .and_then(|metadata| metadata.destination_branch.clone())
        .unwrap_or_else(|| source_ref.clone());
    cache.validate_branch(&destination_branch, runner)?;

    let local_ref = format!("refs/heads/{source_ref}");
    let local_commit = runner
        .git(
            repo,
            &["rev-parse", "--verify", &format!("{local_ref}^{{commit}}")],
        )
        .ok();
    let clone_repository = CloneRepositoryPlan {
        repository_id: repository_id.clone(),
        repository_identity: cache.identity.clone(),
        source_ref: source_ref.clone(),
        destination_branch: destination_branch.clone(),
    };
    let (source_commit, base_commit) = resolve_source_head(SourceHeadRequest {
        cache: &cache,
        repo,
        dir,
        ticket,
        source_ref: &source_ref,
        local_commit: local_commit.as_deref(),
        publication_mode,
        explicitly_selected: explicitly_selected.is_some(),
        clone_repository,
        runner,
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

pub(super) fn resume_from_plan(
    planning_store: &PlanningStore,
    repo: &Path,
    ticket: &str,
    metadata: Option<&crate::artifacts::task_docs::TaskMetadata>,
    plan: &crate::core::implementation::initial_reconciliation::Plan,
    runner: &Runner,
) -> anyhow::Result<TaskSource> {
    let saved = plan
        .clone_repository
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Saved reconciliation plan has no task clone identity"))?;
    anyhow::ensure!(
        saved.source_ref == plan.base,
        "Saved task source reference does not match its reconciliation plan"
    );
    anyhow::ensure!(
        metadata
            .and_then(|metadata| metadata.source_branch.as_deref())
            .is_none_or(|source| source == saved.source_ref)
            && metadata
                .and_then(|metadata| metadata.destination_branch.as_deref())
                .is_none_or(|destination| destination == saved.destination_branch),
        "Task branch intent changed while its starting-point reconciliation was being prepared"
    );
    let cache_path =
        crate::core::implementation::repository_cache::cache_path(&saved.repository_identity)?;
    let cache = RepositoryCache::open_at(
        repo,
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

struct SourceHeadRequest<'a> {
    cache: &'a RepositoryCache,
    repo: &'a Path,
    dir: &'a Path,
    ticket: &'a str,
    source_ref: &'a str,
    local_commit: Option<&'a str>,
    publication_mode: PublicationMode,
    explicitly_selected: bool,
    clone_repository: CloneRepositoryPlan,
    runner: &'a Runner,
}

fn resolve_source_head(request: SourceHeadRequest<'_>) -> anyhow::Result<(String, String)> {
    let SourceHeadRequest {
        cache,
        repo,
        dir,
        ticket,
        source_ref,
        local_commit,
        publication_mode,
        explicitly_selected,
        clone_repository,
        runner,
    } = request;
    let prefer_remote = publication_mode == PublicationMode::AutoPublish && !explicitly_selected;
    let (Some(remote_url), Some(local)) = (cache.fetch_url.as_deref(), local_commit) else {
        if prefer_remote || local_commit.is_none() {
            return refresh_source(cache, source_ref, explicitly_selected, runner);
        }
        let local = import_local_source(cache, repo, source_ref, local_commit.unwrap(), runner)?;
        return Ok((local.clone(), local));
    };
    if prefer_remote {
        return refresh_source(cache, source_ref, explicitly_selected, runner);
    }

    import_local_source(cache, repo, source_ref, local, runner)?;
    let remote_ref = format!("refs/heads/{source_ref}");
    let advertised = runner.git(
        &cache.path,
        &["ls-remote", "--heads", remote_url, &remote_ref],
    )?;
    if advertised.is_empty() {
        return Ok((local.to_owned(), local.to_owned()));
    }
    cache.pin_source(local, runner)?;
    let remote = cache.refresh_branch(source_ref, runner)?;
    match runner.merge_base(&cache.path, local, &remote)? {
        Some(common) if common == local => Ok((remote.clone(), remote)),
        Some(common) if common == remote => Ok((local.to_owned(), local.to_owned())),
        Some(common_base) => {
            let required = crate::core::implementation::initial_reconciliation::support::required_baseline_checks_for_commits(
                &cache.path,
                runner,
                &common_base,
                local,
                &remote,
            )?;
            crate::core::implementation::initial_reconciliation::support::pin_plan_commits(
                &cache.path,
                runner,
                &crate::core::implementation::key_for_ticket(ticket),
                local,
                &remote,
            )?;
            crate::core::implementation::initial_reconciliation::save_clone_plan(
                dir,
                source_ref,
                local,
                &remote,
                &common_base,
                &required,
                clone_repository,
            )?;
            Ok((local.to_owned(), local.to_owned()))
        }
        None => Err(
            crate::core::implementation::initial_reconciliation::support::user_action(format!(
                "Local {source_ref} and origin/{source_ref} have no common history. Both versions are preserved; review the branch histories before implementing."
            )),
        ),
    }
}

fn refresh_source(
    cache: &RepositoryCache,
    source_ref: &str,
    explicitly_selected: bool,
    runner: &Runner,
) -> anyhow::Result<(String, String)> {
    cache
        .refresh_branch(source_ref, runner)
        .map(|commit| (commit.clone(), commit))
        .map_err(|error| {
            if explicitly_selected {
                crate::core::implementation::initial_reconciliation::support::user_action(
                    format!(
                        "Selected source branch '{source_ref}' is unavailable locally and on origin. Refresh the repository and select an existing source branch. {error}"
                    ),
                )
            } else {
                error
            }
        })
}

fn import_local_source(
    cache: &RepositoryCache,
    repo: &Path,
    source_ref: &str,
    local: &str,
    runner: &Runner,
) -> anyhow::Result<String> {
    let imported = cache.import_local_branch(repo, source_ref, local, runner)?;
    anyhow::ensure!(
        imported == local,
        "Source branch moved while its starting commit was being saved; retry to select the new commit"
    );
    Ok(local.to_owned())
}
