use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    MigrationRepositorySource, WorkspaceManager,
    inventory::{Artifact, ArtifactMapping, BranchArtifact},
};

pub(super) struct JournalSource<'a> {
    pub source_commit: &'a str,
    pub source_ref: &'a str,
    pub inventory_sha256: &'a str,
    pub legacy_workspace_id: Option<uuid::Uuid>,
    pub source_repositories: &'a [MigrationRepositorySource],
    pub source_artifacts: &'a [Artifact],
    pub source_branches: &'a [BranchArtifact],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Journal {
    pub schema_version: u32,
    pub workspace_id: uuid::Uuid,
    pub source_root: PathBuf,
    pub source_commit: String,
    pub source_ref: String,
    pub inventory_sha256: String,
    pub phase: String,
    #[serde(default)]
    pub source_repositories: Vec<MigrationRepositorySource>,
    #[serde(default)]
    pub source_artifacts: Vec<ArtifactMapping>,
    #[serde(default)]
    pub source_branches: Vec<BranchArtifact>,
    #[serde(default)]
    pub initial_commit: Option<String>,
}

impl Journal {
    pub fn load_or_create(
        manager: &WorkspaceManager,
        source_root: &Path,
        source: JournalSource<'_>,
    ) -> anyhow::Result<Self> {
        let path = path(manager, source_root);
        if path.is_file() {
            let journal: Self = serde_json::from_slice(&fs::read(path)?)?;
            anyhow::ensure!(journal.schema_version == 1, "unsupported migration journal");
            anyhow::ensure!(
                journal.source_root == source_root,
                "migration source identity mismatch"
            );
            anyhow::ensure!(
                journal.source_commit == source.source_commit
                    && journal.source_ref == source.source_ref,
                "source Git identity changed after migration began; inspect the local migration journal"
            );
            anyhow::ensure!(
                journal.inventory_sha256 == source.inventory_sha256,
                "legacy planning artifacts changed after migration began; start a new reviewed migration"
            );
            anyhow::ensure!(
                source
                    .legacy_workspace_id
                    .is_none_or(|id| journal.workspace_id == id),
                "legacy workspace identity changed after migration began"
            );
            let artifact_mappings = source
                .source_artifacts
                .iter()
                .map(|artifact| artifact.mapping.clone())
                .collect::<Vec<_>>();
            anyhow::ensure!(
                journal.source_repositories == source.source_repositories
                    && journal.source_artifacts == artifact_mappings
                    && journal.source_branches == source.source_branches,
                "migration source mapping changed after the local journal was written"
            );
            return Ok(journal);
        }
        let artifact_mappings = source
            .source_artifacts
            .iter()
            .map(|artifact| artifact.mapping.clone())
            .collect::<Vec<_>>();
        let journal = Self {
            schema_version: 1,
            workspace_id: source
                .legacy_workspace_id
                .unwrap_or_else(uuid::Uuid::new_v4),
            source_root: source_root.to_path_buf(),
            source_commit: source.source_commit.to_owned(),
            source_ref: source.source_ref.to_owned(),
            inventory_sha256: source.inventory_sha256.to_owned(),
            phase: "prepared".into(),
            source_repositories: source.source_repositories.to_vec(),
            source_artifacts: artifact_mappings,
            source_branches: source.source_branches.to_vec(),
            initial_commit: None,
        };
        journal.write(&path)?;
        Ok(journal)
    }

    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::artifacts::atomic_write_bytes(path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

pub(super) fn path(manager: &WorkspaceManager, source_root: &Path) -> PathBuf {
    let digest = Sha256::digest(source_root.to_string_lossy().as_bytes());
    manager
        .home
        .join("projects/.migrations")
        .join(format!("{:x}.json", digest))
}
