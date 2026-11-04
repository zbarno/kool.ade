use super::ProjectManifest;
use crate::artifacts::planning_store::PlanningStore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

const CHECKOUTS_FILE: &str = "config/repository-checkouts.json";

pub struct RepositoryCheckoutRegistration<'a> {
    pub id: &'a str,
    pub role: &'a str,
    pub remote: &'a str,
    pub path: &'a std::path::Path,
}

/// Per-installation checkout associations; paths never enter source Git.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCheckoutMapV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub checkouts: BTreeMap<String, PathBuf>,
}

impl RepositoryCheckoutMapV1 {
    pub fn load(store: &PlanningStore) -> anyhow::Result<Self> {
        let value = match store.read(CHECKOUTS_FILE) {
            Ok(bytes) => {
                let value = serde_json::from_slice::<Self>(&bytes)?;
                anyhow::ensure!(
                    value.schema_version == 1,
                    "Unsupported repository checkout map schema version {}",
                    value.schema_version
                );
                value
            }
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Self::default()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(value)
    }
}

/// Explicitly associates a secondary repository with a local checkout.
pub fn connect_checkout(
    store: &PlanningStore,
    manifest: &ProjectManifest,
    code_root: &std::path::Path,
    repository_id: &str,
    selected_path: &std::path::Path,
    expected_revision: &str,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        repository_id != "root",
        "The primary checkout is already connected"
    );
    let repository = manifest
        .repositories
        .iter()
        .find(|repo| repo.id == repository_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown repository {repository_id}"))?;
    anyhow::ensure!(
        !repository.remote.is_empty(),
        "Repository {repository_id} has no configured remote"
    );
    let canonical = selected_path.canonicalize()?;
    anyhow::ensure!(
        crate::core::gitops::worktree_root(&canonical).as_deref() == Some(canonical.as_path()),
        "Selected path is not a Git worktree root"
    );
    anyhow::ensure!(
        canonical != code_root.canonicalize()?,
        "A secondary checkout cannot alias the primary code repository"
    );
    anyhow::ensure!(
        crate::core::gitops::origin_url(&canonical).as_deref() == Some(repository.remote.as_str()),
        "Selected checkout origin does not match the configured remote for {repository_id}"
    );
    let mut map = RepositoryCheckoutMapV1::load(store)?;
    map.schema_version = 1;
    map.checkouts.insert(repository_id.to_owned(), canonical);
    let (_, revision) = store.transaction_with_revision(
        &[(CHECKOUTS_FILE.to_owned(), serde_json::to_vec_pretty(&map)?)],
        Some(expected_revision),
    )?;
    Ok(revision)
}

/// Registers a secondary repository and its explicit local checkout in one
/// fenced private-store transaction.
pub fn register_repository_checkout(
    store: &PlanningStore,
    current: &ProjectManifest,
    code_root: &std::path::Path,
    registration: RepositoryCheckoutRegistration<'_>,
    expected_revision: &str,
) -> anyhow::Result<(ProjectManifest, String)> {
    let RepositoryCheckoutRegistration {
        id,
        role,
        remote,
        path: selected_path,
    } = registration;
    anyhow::ensure!(
        super::valid_id(id) && id != "root",
        "Invalid secondary repository ID"
    );
    anyhow::ensure!(
        !current.repositories.iter().any(|repo| repo.id == id),
        "Repository {id} is already registered"
    );
    anyhow::ensure!(!role.trim().is_empty(), "Repository role is required");
    anyhow::ensure!(
        super::portable_remote(remote),
        "Repository remote must be a portable Git identity"
    );
    let path = selected_path.canonicalize()?;
    anyhow::ensure!(
        crate::core::gitops::worktree_root(&path).as_deref() == Some(path.as_path()),
        "Selected path is not a Git worktree root"
    );
    anyhow::ensure!(
        path != code_root.canonicalize()?,
        "A secondary checkout cannot alias the primary code repository"
    );
    anyhow::ensure!(
        crate::core::gitops::origin_url(&path).as_deref() == Some(remote),
        "Selected checkout origin does not match the configured remote"
    );

    let mut updated = current.clone();
    updated.repositories.push(super::Repository {
        id: id.to_owned(),
        role: role.trim().to_owned(),
        remote: remote.to_owned(),
        display_name: None,
    });
    updated.validate()?;
    let mut map = RepositoryCheckoutMapV1::load(store)?;
    map.schema_version = 1;
    map.checkouts.insert(id.to_owned(), path);
    let changes = [
        (
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
            serde_json::to_vec_pretty(&updated)?,
        ),
        (CHECKOUTS_FILE.to_owned(), serde_json::to_vec_pretty(&map)?),
    ];
    let (_, revision) = store.transaction_with_revision(&changes, Some(expected_revision))?;
    Ok((updated, revision))
}

#[cfg(test)]
#[path = "checkouts/tests.rs"]
mod tests;
