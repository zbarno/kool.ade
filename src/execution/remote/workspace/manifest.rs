use serde::{Deserialize, Serialize};

use crate::execution::contract::WorkspaceRef;

mod path;
mod stream;
pub(in crate::execution::remote::workspace) use path::{
    is_excluded_component, is_sha256, root_digest, valid_symlink_target, validate_relative_path,
};

pub const WORKSPACE_MANIFEST_VERSION: u16 = 1;
pub const MAX_WORKSPACE_ENTRIES: usize = 100_000;
pub const MAX_WORKSPACE_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_WORKSPACE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_WORKSPACE_EXCLUSIONS: usize = 10_000;
pub const MAX_WORKSPACE_MANIFEST_BYTES: usize = 512 * 1024;
pub const MAX_WORKSPACE_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEntryKindV1 {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEntryV1 {
    pub path: String,
    pub kind: WorkspaceEntryKindV1,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub executable: bool,
    pub symlink_target: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceManifestV1 {
    pub schema_version: u16,
    pub workspace: WorkspaceRef,
    pub snapshot_generation: String,
    pub entries: Vec<WorkspaceEntryV1>,
    /// Relative paths omitted because they may contain credentials or secrets.
    pub excluded_paths: Vec<String>,
    pub total_bytes: u64,
    pub root_sha256: String,
}

impl WorkspaceManifestV1 {
    pub fn capture(
        workspace: WorkspaceRef,
        root: &std::path::Path,
    ) -> Result<Self, crate::execution::errors::ExecutionError> {
        super::scan::capture(workspace, root)
    }

    /// Capture worker output while preserving the generation of its verified
    /// input snapshot. The original Git SHA remains bound in WorkspaceRef; the
    /// worker repository has a separate app-generated baseline commit.
    pub fn capture_result(
        workspace: WorkspaceRef,
        root: &std::path::Path,
        snapshot_generation: String,
    ) -> Result<Self, crate::execution::errors::ExecutionError> {
        super::scan::capture_result(workspace, root, snapshot_generation)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.schema_version == WORKSPACE_MANIFEST_VERSION);
        self.workspace.validate()?;
        let generation = uuid::Uuid::parse_str(&self.snapshot_generation)?;
        anyhow::ensure!(
            generation.hyphenated().to_string() == self.snapshot_generation,
            "workspace snapshot generation is not canonical"
        );
        anyhow::ensure!(
            self.entries.len() <= MAX_WORKSPACE_ENTRIES
                && self.excluded_paths.len() <= MAX_WORKSPACE_EXCLUSIONS,
            "workspace manifest exceeds the entry limit"
        );
        anyhow::ensure!(self.total_bytes <= MAX_WORKSPACE_BYTES);
        let mut total = 0_u64;
        let mut previous: Option<&str> = None;
        let mut folded = std::collections::HashSet::new();
        for entry in &self.entries {
            validate_relative_path(&entry.path)?;
            anyhow::ensure!(
                !entry.path.split('/').any(is_excluded_component),
                "workspace manifest includes a protected path"
            );
            if let Some(previous) = previous {
                anyhow::ensure!(
                    previous < entry.path.as_str(),
                    "workspace paths are not sorted"
                );
            }
            previous = Some(&entry.path);
            anyhow::ensure!(
                folded.insert(entry.path.to_lowercase()),
                "workspace has case-insensitive path collisions"
            );
            match entry.kind {
                WorkspaceEntryKindV1::Directory => anyhow::ensure!(
                    entry.size_bytes == 0
                        && entry.sha256.is_none()
                        && entry.symlink_target.is_none()
                ),
                WorkspaceEntryKindV1::File => {
                    anyhow::ensure!(
                        entry.size_bytes <= MAX_WORKSPACE_FILE_BYTES
                            && entry.sha256.as_deref().is_some_and(is_sha256)
                            && entry.symlink_target.is_none(),
                        "workspace file metadata is invalid"
                    );
                    total = total
                        .checked_add(entry.size_bytes)
                        .ok_or_else(|| anyhow::anyhow!("workspace size overflow"))?;
                }
                WorkspaceEntryKindV1::Symlink => anyhow::ensure!(
                    entry.size_bytes == 0
                        && entry.sha256.is_none()
                        && entry
                            .symlink_target
                            .as_deref()
                            .is_some_and(|target| { valid_symlink_target(&entry.path, target) }),
                    "workspace symlink metadata is invalid"
                ),
            }
        }
        let mut previous_excluded: Option<&str> = None;
        let mut excluded_folded = std::collections::HashSet::new();
        for excluded in &self.excluded_paths {
            validate_relative_path(excluded)?;
            anyhow::ensure!(
                excluded.split('/').any(is_excluded_component),
                "workspace manifest has an invalid exclusion"
            );
            if let Some(previous) = previous_excluded {
                anyhow::ensure!(
                    previous < excluded.as_str(),
                    "workspace exclusions are not sorted"
                );
            }
            previous_excluded = Some(excluded);
            anyhow::ensure!(
                excluded_folded.insert(excluded.to_lowercase()),
                "workspace exclusions contain case-insensitive duplicates"
            );
        }
        anyhow::ensure!(
            total == self.total_bytes,
            "workspace manifest byte total is inconsistent"
        );
        anyhow::ensure!(
            is_sha256(&self.root_sha256),
            "workspace root digest is invalid"
        );
        anyhow::ensure!(
            root_digest(&self.entries, &self.excluded_paths)? == self.root_sha256,
            "workspace root digest does not match its entries"
        );
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= MAX_WORKSPACE_MANIFEST_BYTES,
            "workspace manifest metadata exceeds the size limit"
        );
        Ok(())
    }

    /// Streams one file against this validated snapshot without buffering it.
    /// Chunks are provisional until this method returns successfully; callers
    /// must not commit a received file after a late hash or I/O failure.
    pub fn stream_file(
        &self,
        root: &std::path::Path,
        relative_path: &str,
        on_chunk: impl FnMut(&[u8]) -> Result<(), crate::execution::errors::ExecutionError>,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        stream::stream_file(self, root, relative_path, on_chunk)
    }
}
