use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationConflictPreview {
    pub path: String,
    pub record_id: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreview {
    pub source_root: PathBuf,
    pub workspace_id: Option<uuid::Uuid>,
    pub source_commit: String,
    pub source_ref: String,
    pub inventory_digest: String,
    pub file_count: usize,
    pub record_ids: Vec<String>,
    pub collisions: Vec<MigrationConflictPreview>,
    /// Legacy artifacts remain available here after migration for recovery.
    pub backup_path: PathBuf,
    pub exclusions: Vec<String>,
}

pub(super) fn build(code_checkout: &Path) -> anyhow::Result<MigrationPreview> {
    super::stage::preview(code_checkout)
}
