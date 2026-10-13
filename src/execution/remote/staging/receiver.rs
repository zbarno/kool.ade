//! Durable chunk reception with source binding and restart recovery.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::{TransferChunkV1, TransferManifestV1, TransferPayloadV1};
use crate::execution::contract::WorkspaceRef;

mod materialize;
mod result;
mod storage;
mod validation;
use materialize::assemble;
use storage::{ChunkRecord, transfer_directory};
use validation::{ensure_not_finalized, validate_records};

const MAX_CHUNK_RECORDS: usize = 150_000;
const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CompletionRecord {
    schema_version: u8,
    transfer_id: String,
    payload_sha256: String,
}

pub struct StagingReceiver {
    directory: PathBuf,
    manifest: TransferManifestV1,
    records: BTreeMap<(String, u64), ChunkRecord>,
}

/// A controller-side result tree that has passed transfer, file, and manifest
/// integrity checks and is bound to the original task identity and source SHA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedWorkspaceResult {
    workspace: WorkspaceRef,
    manifest: crate::execution::remote::WorkspaceManifestV1,
    snapshot_generation: String,
    root_sha256: String,
    transfer_id: String,
    materialized_path: PathBuf,
}

impl VerifiedWorkspaceResult {
    pub fn workspace(&self) -> &WorkspaceRef {
        &self.workspace
    }

    pub fn manifest(&self) -> &crate::execution::remote::WorkspaceManifestV1 {
        &self.manifest
    }

    pub fn snapshot_generation(&self) -> &str {
        &self.snapshot_generation
    }

    pub fn root_sha256(&self) -> &str {
        &self.root_sha256
    }

    pub fn transfer_id(&self) -> &str {
        &self.transfer_id
    }

    pub fn materialized_path(&self) -> &Path {
        &self.materialized_path
    }
}

impl StagingReceiver {
    /// Create or resume a transfer only when both authenticated identities match.
    pub fn begin(
        root: impl AsRef<Path>,
        manifest: TransferManifestV1,
        expected_source: &str,
        expected_destination: &str,
    ) -> anyhow::Result<Self> {
        manifest.validate()?;
        anyhow::ensure!(
            manifest.source_identity == expected_source,
            "transfer source identity mismatch"
        );
        anyhow::ensure!(
            manifest.destination_identity == expected_destination,
            "transfer destination identity mismatch"
        );
        let root = root.as_ref().to_path_buf();
        storage::ensure_private_dir(&root)?;
        let directory = transfer_directory(&root, &manifest.transfer_id, true)?;
        let manifest_path = directory.join("manifest.json");
        let encoded = serde_json::to_vec(&manifest)?;
        match crate::artifacts::atomic_create_bytes(&manifest_path, &encoded) {
            Ok(()) => storage::set_private_file(&manifest_path)?,
            Err(error) if manifest_path.exists() => {
                let previous: TransferManifestV1 =
                    serde_json::from_slice(&storage::read_bounded(&manifest_path, 512 * 1024)?)?;
                anyhow::ensure!(
                    previous == manifest,
                    "transfer ID is already bound to different source data"
                );
                storage::set_private_file(&manifest_path)?;
                let _ = error;
            }
            Err(error) => return Err(error.into()),
        }
        let records = storage::read_journal(&directory)?;
        validate_records(&manifest, &directory, &records)?;
        Ok(Self {
            directory,
            manifest,
            records,
        })
    }

    pub fn manifest(&self) -> &TransferManifestV1 {
        &self.manifest
    }

    /// Persist one bounded chunk. Duplicate bytes at the same offset are safe;
    /// a conflicting replay is rejected and never overwrites stored content.
    pub fn accept_chunk(&mut self, chunk: &TransferChunkV1) -> anyhow::Result<()> {
        chunk.validate()?;
        anyhow::ensure!(
            chunk.transfer_id == self.manifest.transfer_id,
            "chunk belongs to another transfer"
        );
        let file = self
            .manifest
            .files()
            .into_iter()
            .find(|file| file.path == chunk.path)
            .ok_or_else(|| anyhow::anyhow!("chunk path is not in the transfer manifest"))?;
        let end = chunk
            .offset
            .checked_add(chunk.bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("chunk offset overflow"))?;
        anyhow::ensure!(end <= file.size_bytes, "chunk exceeds declared file size");
        anyhow::ensure!(
            chunk.bytes.len() == super::MAX_TRANSFER_CHUNK_BYTES || end == file.size_bytes,
            "non-final chunk is truncated"
        );

        let lock_path = self.directory.join("transfer.lock");
        let lock = storage::open_private_lock(&lock_path)?;
        lock.lock_exclusive()?;
        ensure_not_finalized(&self.directory)?;
        self.records = storage::read_journal(&self.directory)?;
        let result = self.accept_locked(chunk);
        FileExt::unlock(&lock)?;
        result
    }

    fn accept_locked(&mut self, chunk: &TransferChunkV1) -> anyhow::Result<()> {
        let key = (chunk.path.clone(), chunk.offset);
        let record = ChunkRecord::from_chunk(chunk);
        if let Some(previous) = self.records.get(&key) {
            anyhow::ensure!(
                previous == &record,
                "transfer chunk replay conflicts with durable data"
            );
            storage::verify_chunk_file(&self.directory, chunk)?;
            return Ok(());
        }
        let path_hash = storage::path_digest(&chunk.path);
        let chunk_path = storage::chunk_path(&self.directory, &path_hash, chunk.offset);
        storage::prepare_chunk_directory(chunk_path.parent().expect("chunk path has parent"))?;
        match crate::artifacts::atomic_create_bytes(&chunk_path, &chunk.bytes) {
            Ok(()) => storage::set_private_file(&chunk_path)?,
            Err(error) if chunk_path.exists() => {
                storage::verify_chunk_file(&self.directory, chunk)?;
                storage::set_private_file(&chunk_path)?;
                let _ = error;
            }
            Err(error) => return Err(error.into()),
        }
        let next_count = self
            .records
            .len()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("transfer record count overflow"))?;
        anyhow::ensure!(
            next_count <= MAX_CHUNK_RECORDS,
            "transfer has too many chunks"
        );
        storage::append_journal(&self.directory, &record)?;
        self.records.insert(key, record);
        Ok(())
    }

    /// Verify all files and atomically publish a private materialized tree.
    pub fn finalize(&self) -> anyhow::Result<PathBuf> {
        let lock = storage::open_private_lock(&self.directory.join("transfer.lock"))?;
        lock.lock_exclusive()?;
        let result = self.finalize_locked();
        FileExt::unlock(&lock)?;
        result
    }

    fn finalize_locked(&self) -> anyhow::Result<PathBuf> {
        let complete = self.directory.join("complete.json");
        if fs::symlink_metadata(&complete).is_ok() {
            let record: CompletionRecord =
                serde_json::from_slice(&storage::read_bounded(&complete, 4096)?)?;
            anyhow::ensure!(
                record.transfer_id == self.manifest.transfer_id
                    && record.payload_sha256 == self.manifest.payload_sha256,
                "completed transfer marker does not match"
            );
            let materialized = self.directory.join("materialized");
            let metadata = fs::symlink_metadata(&materialized)?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "completed materialized tree is not trusted"
            );
            return Ok(materialized);
        }
        let materialized = assemble(&self.directory, &self.manifest, &self.records)?;
        let record = CompletionRecord {
            schema_version: 1,
            transfer_id: self.manifest.transfer_id.clone(),
            payload_sha256: self.manifest.payload_sha256.clone(),
        };
        let bytes = serde_json::to_vec(&record)?;
        crate::artifacts::atomic_create_bytes(&complete, &bytes)?;
        storage::set_private_file(&complete)?;
        Ok(materialized)
    }

    pub fn received_chunk_count(&self) -> usize {
        self.records.len()
    }
}

#[cfg(test)]
#[path = "receiver/tests.rs"]
mod tests;
