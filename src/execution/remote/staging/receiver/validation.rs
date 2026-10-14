use std::{collections::BTreeMap, fs, path::Path};

use sha2::Digest;

use super::{ChunkRecord, MAX_CHUNK_RECORDS, storage};
use crate::execution::remote::staging::TransferManifestV1;

pub(super) fn ensure_not_finalized(directory: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(directory.join("complete.json")) {
        Ok(_) => anyhow::bail!("transfer is already finalized"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn validate_records(
    manifest: &TransferManifestV1,
    directory: &Path,
    records: &BTreeMap<(String, u64), ChunkRecord>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        records.len() <= MAX_CHUNK_RECORDS,
        "transfer has too many chunks"
    );
    let files = manifest.files();
    for record in records.values() {
        let file = files
            .iter()
            .find(|file| file.path == record.path)
            .ok_or_else(|| anyhow::anyhow!("transfer journal names an unknown file"))?;
        let end = record
            .offset
            .checked_add(record.size_bytes as u64)
            .ok_or_else(|| anyhow::anyhow!("transfer journal offset overflows"))?;
        anyhow::ensure!(
            record.offset % super::super::MAX_TRANSFER_CHUNK_BYTES as u64 == 0
                && record.size_bytes > 0
                && record.size_bytes <= super::super::MAX_TRANSFER_CHUNK_BYTES
                && end <= file.size_bytes
                && (record.size_bytes == super::super::MAX_TRANSFER_CHUNK_BYTES
                    || end == file.size_bytes)
                && record.sha256.len() == 64
                && record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "transfer journal chunk metadata is invalid"
        );
        let chunk_path = storage::chunk_path(
            directory,
            &storage::path_digest(&record.path),
            record.offset,
        );
        let chunk =
            storage::read_bounded(&chunk_path, super::super::MAX_TRANSFER_CHUNK_BYTES as u64)?;
        anyhow::ensure!(
            chunk.len() == record.size_bytes
                && format!("{:x}", sha2::Sha256::digest(&chunk)) == record.sha256,
            "durable transfer chunk is corrupt"
        );
    }
    Ok(())
}
