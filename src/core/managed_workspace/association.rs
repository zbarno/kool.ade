use std::{collections::HashSet, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCheckout {
    pub repository_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAssociation {
    pub schema_version: u32,
    #[serde(with = "super::manifest::canonical_uuid")]
    pub workspace_id: uuid::Uuid,
    pub planning_repo_path: PathBuf,
    #[serde(default)]
    pub code_checkouts: Vec<CodeCheckout>,
}

impl WorkspaceAssociation {
    pub fn new(workspace_id: uuid::Uuid, planning_repo_path: PathBuf) -> Self {
        Self {
            schema_version: 1,
            workspace_id,
            planning_repo_path,
            code_checkouts: Vec::new(),
        }
    }

    pub fn validate(&self, expected_id: uuid::Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported workspace association schema"
        );
        anyhow::ensure!(
            self.workspace_id == expected_id,
            "workspace association identity mismatch"
        );
        anyhow::ensure!(
            self.planning_repo_path.is_absolute(),
            "workspace planning repository path must be absolute"
        );
        let mut ids = HashSet::new();
        for checkout in &self.code_checkouts {
            anyhow::ensure!(
                !checkout.repository_id.is_empty() && ids.insert(&checkout.repository_id),
                "workspace association contains a duplicate or empty repository ID"
            );
            anyhow::ensure!(
                checkout.path.is_absolute(),
                "code checkout path must be absolute"
            );
        }
        Ok(())
    }

    pub fn read(path: &std::path::Path, expected_id: uuid::Uuid) -> anyhow::Result<Self> {
        let association: Self = serde_json::from_slice(&fs::read(path)?)?;
        association.validate(expected_id)?;
        Ok(association)
    }

    pub fn write(&self, path: &std::path::Path) -> anyhow::Result<()> {
        self.validate(self.workspace_id)?;
        let bytes = serde_json::to_vec_pretty(self)?;
        crate::artifacts::atomic_write_bytes(path, &bytes)
    }
}
