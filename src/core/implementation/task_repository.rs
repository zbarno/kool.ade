use super::{Implementation, Runner, TaskRepositoryKind, key};
use crate::core::implementation::repository_cache::RepositoryCache;
use std::path::{Path, PathBuf};

pub(super) fn task_path(
    planning_root: &Path,
    repository_id: &str,
    ticket: &str,
) -> anyhow::Result<PathBuf> {
    let project_id = crate::persistence::project_slug(&planning_root.canonicalize()?);
    allocated_path(&project_id, repository_id, &key(ticket))
}

pub(super) fn allocated_path(
    project_id: &str,
    repository_id: &str,
    allocation: &str,
) -> anyhow::Result<PathBuf> {
    validate_allocation_key(allocation)?;
    Ok(repository_root(project_id, repository_id)?.join(allocation))
}

pub(super) fn project_id(planning_root: &Path) -> anyhow::Result<String> {
    Ok(crate::persistence::project_slug(
        &planning_root.canonicalize()?,
    ))
}

pub(super) fn allocation_key(state: &Implementation) -> String {
    state
        .task_repository_allocation_key
        .clone()
        .unwrap_or_else(|| key(&state.ticket))
}

pub(super) fn cache_for_state(
    repo: &Path,
    planning_root: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<RepositoryCache> {
    validate_clone_path(state)?;
    anyhow::ensure!(
        state.task_repository_kind == TaskRepositoryKind::Clone,
        "Legacy task repository does not use the repository cache"
    );
    let repository_id = state
        .repository_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved task repository identity is missing"))?;
    let (text, _, metadata) = super::read_ticket_and_identity(planning_root, &state.ticket)?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_root)?;
    anyhow::ensure!(
        super::task_repository_id(&text, metadata.as_ref(), &manifest)? == repository_id,
        "Task repository mapping changed; review the saved task before resuming"
    );
    let cache_path = state
        .repository_cache
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved repository cache path is missing"))?;
    let cache =
        RepositoryCache::open_at(repo, planning_root, repository_id, Some(cache_path), runner)?;
    anyhow::ensure!(
        state.repository_identity.as_deref() == Some(cache.identity.as_str()),
        "Repository identity changed since task start; the saved clone is preserved"
    );
    Ok(cache)
}

pub(super) fn validate_task_path(
    planning_root: &Path,
    state: &Implementation,
) -> anyhow::Result<()> {
    let repository_id = state
        .repository_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved task repository identity is missing"))?;
    let project_id = project_id(planning_root)?;
    anyhow::ensure!(state.project_id.as_deref() == Some(project_id.as_str()));
    let allocation = allocation_key(state);
    validate_allocation_key(&allocation)?;
    let expected = repository_root(&project_id, repository_id)?.join(allocation);
    anyhow::ensure!(
        state.task_repository == expected,
        "Saved task repository path differs from its Kool.ad/e allocation"
    );
    Ok(())
}

pub(super) fn validate_clone_path(state: &Implementation) -> anyhow::Result<()> {
    validate_clone_path_at(state, &state.task_repository)
}

pub(super) fn validate_clone_path_at(state: &Implementation, path: &Path) -> anyhow::Result<()> {
    let repository_id = state
        .repository_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved task repository identity is missing"))?;
    let project_id = state
        .project_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Saved task project identity is missing"))?;
    let root = repository_root(project_id, repository_id)?;
    let key = allocation_key(state);
    validate_allocation_key(&key)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Saved task repository path is invalid"))?;
    anyhow::ensure!(
        path.parent() == Some(root.as_path())
            && (name == key || valid_integration_name(name, &key))
            && state.task_repositories.contains(&path.to_path_buf()),
        "Saved task repository path differs from its Kool.ad/e allocation"
    );
    Ok(())
}

pub(super) fn branch_for_path(state: &Implementation, path: &Path) -> anyhow::Result<String> {
    validate_clone_path_at(state, path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Saved task repository path is invalid"))?;
    let key = allocation_key(state);
    validate_allocation_key(&key)?;
    if name == key {
        Ok(format!("koolade/{key}"))
    } else {
        let suffix = name
            .strip_prefix(&format!("{key}-integration-"))
            .ok_or_else(|| anyhow::anyhow!("Invalid integration repository name"))?;
        Ok(format!("koolade/integration/{key}/{suffix}"))
    }
}

fn repository_root(project_id: &str, repository_id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !project_id.is_empty()
            && project_id.len() <= 128
            && project_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "Invalid task project identity"
    );
    anyhow::ensure!(
        !repository_id.is_empty()
            && repository_id.len() <= 40
            && repository_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        "Invalid task repository ID"
    );
    Ok(crate::persistence::project_dir(project_id)
        .join("task-repositories")
        .join(repository_id))
}

fn valid_integration_name(name: &str, task_key: &str) -> bool {
    name.strip_prefix(&format!("{task_key}-integration-"))
        .is_some_and(|suffix| {
            suffix.len() == 12 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn validate_allocation_key(key: &str) -> anyhow::Result<()> {
    let (slug, hash) = key
        .rsplit_once('-')
        .ok_or_else(|| anyhow::anyhow!("Invalid task repository allocation key"))?;
    anyhow::ensure!(
        !slug.is_empty()
            && slug
                .chars()
                .all(|character| character.is_alphanumeric() || character == '-')
            && hash.len() == 16
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid task repository allocation key"
    );
    Ok(())
}
