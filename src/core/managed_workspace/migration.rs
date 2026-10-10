//! Explicit migration from an embedded planning root to a managed repository.
mod inventory;
mod journal;
mod stage;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ManagedWorkspace, WorkspaceManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub schema_version: u32,
    pub workspace_id: uuid::Uuid,
    pub source_commit: String,
    pub source_ref: String,
    pub source_repositories: Vec<MigrationRepositorySource>,
    pub source_artifacts: Vec<inventory::ArtifactMapping>,
    pub source_branches: Vec<inventory::BranchArtifact>,
    pub generated_artifacts: Vec<GeneratedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRepositorySource {
    pub repository_id: String,
    pub source_commit: String,
    pub source_ref: String,
    pub artifact_count: usize,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedArtifact {
    pub target_path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackOutcome {
    pub detached: bool,
    pub warning: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MigrationFailurePoint {
    JournalWritten,
    InitialCommitCreated,
    RepositoryRenamed,
    AssociationWritten,
}

impl WorkspaceManager {
    /// Explicitly migrate one legacy code checkout. The source tree is only
    /// read; the associated planning repository is staged and verified first.
    pub fn migrate_legacy_checkout(
        &self,
        code_checkout: &Path,
    ) -> anyhow::Result<ManagedWorkspace> {
        stage::migrate(self, code_checkout)
    }

    #[cfg(test)]
    pub(super) fn migrate_legacy_checkout_with_failure(
        &self,
        code_checkout: &Path,
        point: MigrationFailurePoint,
    ) -> anyhow::Result<ManagedWorkspace> {
        stage::migrate_with_failure(self, code_checkout, point)
    }

    #[cfg(test)]
    pub(super) fn migrate_legacy_checkout_with_registered_sources(
        &self,
        code_checkout: &Path,
        local_checkouts: &std::collections::BTreeMap<String, PathBuf>,
    ) -> anyhow::Result<ManagedWorkspace> {
        stage::migrate_with_registered_sources(self, code_checkout, local_checkouts)
    }

    /// Detach a migrated checkout after an explicit operator request. The
    /// planning repository remains available; later planning changes require
    /// export before the legacy checkout can safely become authoritative again.
    pub fn rollback_legacy_migration(
        &self,
        code_checkout: &Path,
    ) -> anyhow::Result<RollbackOutcome> {
        stage::rollback(self, code_checkout)
    }

    /// Find the locally associated managed workspace for a code checkout.
    pub fn workspace_for_checkout(
        &self,
        code_checkout: &Path,
    ) -> anyhow::Result<Option<ManagedWorkspace>> {
        stage::workspace_for_checkout(self, code_checkout)
    }
}

pub(super) fn canonical_code_checkout(path: &Path) -> anyhow::Result<PathBuf> {
    let path = path.canonicalize()?;
    anyhow::ensure!(
        super::git::is_work_tree(&path),
        "migration source must be a Git working tree"
    );
    let root = super::git::repository_root(&path)?;
    anyhow::ensure!(
        root == path,
        "select the code repository root for migration"
    );
    Ok(path)
}
