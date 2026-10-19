use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{
    MAX_PRIVATE_FILE_CHUNK_BYTES, RemotePrivateFileGrantV1, canonical_uuid, digest,
    validate_private_path,
};

/// Non-secret metadata carried in the JSON header of a private-file frame.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrivateFileChunkHeaderV1 {
    pub grant_id: String,
    pub path: String,
    pub offset: u64,
    pub byte_count: u32,
    pub sha256: String,
}

impl PrivateFileChunkHeaderV1 {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        canonical_uuid(&self.grant_id)?;
        validate_private_path(&self.path)?;
        anyhow::ensure!(
            self.byte_count > 0
                && self.byte_count as usize <= MAX_PRIVATE_FILE_CHUNK_BYTES
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "private file chunk header is invalid"
        );
        Ok(())
    }

    pub(crate) fn into_chunk(
        self,
        bytes: Zeroizing<Vec<u8>>,
    ) -> anyhow::Result<PrivateFileChunkV1> {
        self.validate()?;
        anyhow::ensure!(bytes.len() == self.byte_count as usize);
        let chunk = PrivateFileChunkV1 {
            grant_id: self.grant_id,
            path: self.path,
            offset: self.offset,
            bytes,
            sha256: self.sha256,
        };
        chunk.validate()?;
        Ok(chunk)
    }
}

/// Secret-bearing chunk data. It deliberately has no `Debug` implementation.
#[derive(PartialEq, Eq)]
pub(crate) struct PrivateFileChunkV1 {
    pub(crate) grant_id: String,
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) bytes: Zeroizing<Vec<u8>>,
    pub(crate) sha256: String,
}

impl PrivateFileChunkV1 {
    pub(crate) fn new(
        grant_id: String,
        path: String,
        offset: u64,
        bytes: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let sha256 = digest(&bytes);
        let chunk = Self {
            grant_id,
            path,
            offset,
            bytes: Zeroizing::new(bytes),
            sha256,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    pub(crate) fn header(&self) -> anyhow::Result<PrivateFileChunkHeaderV1> {
        let byte_count = u32::try_from(self.bytes.len())?;
        let header = PrivateFileChunkHeaderV1 {
            grant_id: self.grant_id.clone(),
            path: self.path.clone(),
            offset: self.offset,
            byte_count,
            sha256: self.sha256.clone(),
        };
        header.validate()?;
        Ok(header)
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        canonical_uuid(&self.grant_id)?;
        validate_private_path(&self.path)?;
        anyhow::ensure!(
            !self.bytes.is_empty()
                && self.bytes.len() <= MAX_PRIVATE_FILE_CHUNK_BYTES
                && self.sha256 == digest(&self.bytes),
            "private file chunk is empty, oversized, or has an invalid digest"
        );
        Ok(())
    }

    pub(crate) fn validate_for_grant(
        &self,
        grant: &RemotePrivateFileGrantV1,
        now_unix_ms: u64,
    ) -> anyhow::Result<()> {
        grant.validate_at(now_unix_ms)?;
        self.validate()?;
        anyhow::ensure!(self.grant_id == grant.grant_id);
        let file = grant
            .files
            .iter()
            .find(|file| file.path == self.path)
            .ok_or_else(|| anyhow::anyhow!("private file chunk path is outside its grant"))?;
        let end = self
            .offset
            .checked_add(self.bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("private file chunk range overflow"))?;
        anyhow::ensure!(
            end <= file.size_bytes,
            "private file chunk exceeds its grant size"
        );
        Ok(())
    }
}
