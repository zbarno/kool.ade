use super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::implementation::repository_cache::RepositoryCache;
use std::fs;

mod copy;
mod migration;
mod snapshot;
use migration::migrate_one;

#[derive(Clone, Copy)]
pub(super) struct MigrationRequest<'a> {
    pub(super) planning_store: &'a PlanningStore,
    pub(super) state_root: &'a Path,
    pub(super) repo: &'a Path,
    pub(super) text: &'a str,
    pub(super) metadata: Option<&'a crate::artifacts::task_docs::TaskMetadata>,
    pub(super) dir: &'a Path,
    pub(super) runner: &'a Runner,
}

pub(super) fn migrate(
    request: MigrationRequest<'_>,
    mut state: Implementation,
) -> anyhow::Result<Implementation> {
    let original_repository = state.task_repository.clone();
    let mut migrated = state.clone();
    let result = migrate_inner(request, &mut migrated);
    if let Err(error) = result {
        state.status = ImplementationStatus::Blocked;
        state.detail = format!(
            "Legacy task repository migration needs attention. The original workspace is preserved at {}. {error:#}",
            original_repository.display()
        );
        save(request.dir, &state)?;
        return Err(anyhow::Error::new(
            crate::core::implementation::status::FailureCause(
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::InvalidSavedState,
                    crate::core::implementation::RecoveryDisposition::UserAction,
                    state.detail.clone(),
                ),
            ),
        ));
    }
    Ok(migrated)
}

fn migrate_inner(request: MigrationRequest<'_>, state: &mut Implementation) -> anyhow::Result<()> {
    let MigrationRequest {
        planning_store,
        state_root,
        repo,
        text,
        metadata,
        dir,
        runner,
    } = request;
    let allocation = task_repository::allocation_key(state);
    let mut reconciliation_plan = initial_reconciliation::load_plan(dir)?;
    if let Some(plan) = reconciliation_plan.as_ref()
        && plan.clone_repository.is_none()
    {
        super::legacy_plan::attach_clone_identity(
            planning_store,
            repo,
            &allocation,
            text,
            metadata,
            dir,
            plan,
            runner,
        )?;
        reconciliation_plan = initial_reconciliation::load_plan(dir)?;
    }
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_store)?;
    let repository_id = crate::core::implementation::task_repository_id(text, metadata, &manifest)?;
    let cache = RepositoryCache::open(repo, planning_store, &repository_id, runner)?;
    let identity =
        crate::core::implementation::repository_cache::load_task_git_identity(dir, repo, runner)?;
    let project_id = task_repository::project_id(state_root)?;
    let main_target = task_repository::allocated_path(&project_id, &repository_id, &allocation)?;
    let target = if state.task_repository_kind == TaskRepositoryKind::LegacyWorktree
        && state.branch == format!("koolade/{allocation}")
    {
        main_target
    } else {
        let suffix = state
            .base_commit
            .get(..12)
            .ok_or_else(|| anyhow::anyhow!("Legacy integration commit identity is invalid"))?;
        task_repository::allocated_path(&project_id, &repository_id, &allocation)?
            .with_file_name(format!("{allocation}-integration-{suffix}"))
    };
    migrate_one(
        repo,
        dir,
        state,
        &repository_id,
        &project_id,
        &cache,
        &identity,
        target.clone(),
        reconciliation_plan
            .as_ref()
            .map(|plan| plan.local_commit.as_str()),
        runner,
    )?;

    let mut repositories = vec![target];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !task_repository::is_integration_state_dir(&entry.file_name()) {
            continue;
        }
        anyhow::ensure!(
            entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink(),
            "Saved integration evidence is not a real directory; preserve it for review"
        );
        let integration_dir = entry.path();
        let record_path = integration_dir.join("state.json");
        let file_meta = fs::symlink_metadata(&record_path)?;
        anyhow::ensure!(
            file_meta.is_file() && !file_meta.file_type().is_symlink(),
            "Saved integration state is not a regular file; preserve it for review"
        );
        let mut integration = read_state_file(&record_path)?;
        if integration.task_repository_kind == TaskRepositoryKind::LegacyWorktree {
            crate::core::implementation::repository_cache::save_task_git_identity(
                &integration_dir,
                &identity,
            )?;
            let suffix = integration
                .base_commit
                .get(..12)
                .ok_or_else(|| anyhow::anyhow!("Legacy integration commit identity is invalid"))?;
            let target = task_repository::allocated_path(
                &project_id,
                &repository_id,
                &task_repository::allocation_key(&integration),
            )?
            .with_file_name(format!(
                "{}-integration-{suffix}",
                task_repository::allocation_key(&integration)
            ));
            migrate_one(
                repo,
                &integration_dir,
                &mut integration,
                &repository_id,
                &project_id,
                &cache,
                &identity,
                target.clone(),
                None,
                runner,
            )?;
            save(&integration_dir, &integration)?;
        }
        repositories.extend(integration.task_repositories.iter().cloned());
    }
    repositories.sort();
    repositories.dedup();
    state.task_repositories = repositories;
    state.repository_id = Some(repository_id);
    state.project_id = Some(project_id);
    state.repository_identity = Some(cache.identity.clone());
    state.push_repository = cache.push_identity_url.clone();
    state.repository_cache = Some(cache.path.clone());
    if state.source_ref.is_none() {
        state.source_ref = Some(
            state
                .source_branch
                .clone()
                .unwrap_or_else(|| state.base.clone()),
        );
    }
    state
        .source_commit
        .get_or_insert_with(|| state.base_commit.clone());
    state.task_repository_ready = true;
    save(dir, state)
}
