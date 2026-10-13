use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TRANSFER_CHUNK_BYTES: usize = 48 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransferChunkV1 {
    pub transfer_id: String,
    pub path: String,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl TransferChunkV1 {
    pub fn new(
        transfer_id: impl Into<String>,
        path: impl Into<String>,
        offset: u64,
        bytes: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let chunk = Self {
            transfer_id: transfer_id.into(),
            path: path.into(),
            offset,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let id = uuid::Uuid::parse_str(&self.transfer_id)?;
        anyhow::ensure!(
            id.hyphenated().to_string() == self.transfer_id,
            "transfer chunk ID is invalid"
        );
        super::manifest::validate_transfer_path(&self.path)?;
        anyhow::ensure!(
            !self.bytes.is_empty() && self.bytes.len() <= MAX_TRANSFER_CHUNK_BYTES,
            "transfer chunk size is invalid"
        );
        anyhow::ensure!(
            self.offset.is_multiple_of(MAX_TRANSFER_CHUNK_BYTES as u64),
            "transfer chunk offset is unaligned"
        );
        anyhow::ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "transfer chunk digest is invalid"
        );
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&self.bytes)) == self.sha256,
            "transfer chunk digest does not match"
        );
        Ok(())
    }
}
