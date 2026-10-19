//! Exact-file capabilities for private data sent to one enrolled remote worker.

use crate::execution::contract::WorkspaceRef;
use serde::{Deserialize, Serialize};

pub const MAX_PRIVATE_FILE_GRANT_FILES: usize = 64;
pub const MAX_PRIVATE_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PRIVATE_FILE_CHUNK_BYTES: usize = 48 * 1024;
pub const MAX_PRIVATE_FILE_GRANT_TTL_MS: u64 = 15 * 60 * 1000;

mod chunk;
mod source;
pub(crate) use chunk::PrivateFileChunkHeaderV1;
pub(crate) use chunk::PrivateFileChunkV1;
pub(crate) use source::{
    AuthorizedPrivateFileBytes, discover_private_files, read_authorized_private_file,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivateDataClassV1 {
    ProjectConfiguration,
    CredentialFile,
    Certificate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivateFileSpecV1 {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub data_class: PrivateDataClassV1,
}

/// User-confirmed, immutable authority for one exact file set and execution.
/// The receipt contains metadata only; file bytes use `PrivateFileChunkV1`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemotePrivateFileGrantV1 {
    pub grant_id: String,
    pub controller_id: String,
    pub operator_id: String,
    pub worker_id: String,
    pub worker_host_key_fingerprint: String,
    pub workspace: WorkspaceRef,
    pub confirmed_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub files: Vec<PrivateFileSpecV1>,
}

impl RemotePrivateFileGrantV1 {
    pub fn validate_at(&self, now_unix_ms: u64) -> anyhow::Result<()> {
        canonical_uuid(&self.grant_id)?;
        canonical_uuid(&self.controller_id)?;
        canonical_uuid(&self.worker_id)?;
        anyhow::ensure!(
            !self.operator_id.trim().is_empty()
                && self.operator_id.len() <= 128
                && !self.operator_id.chars().any(char::is_control),
            "private file grant operator identity is invalid"
        );
        self.workspace.validate()?;
        anyhow::ensure!(
            self.worker_host_key_fingerprint
                .strip_prefix("SHA256:")
                .is_some_and(|encoded| {
                    encoded.len() == 43
                        && encoded
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"+/".contains(&byte))
                }),
            "private file grant worker identity is invalid"
        );
        anyhow::ensure!(
            self.confirmed_at_unix_ms > 0
                && self.confirmed_at_unix_ms <= now_unix_ms
                && self.expires_at_unix_ms > now_unix_ms
                && self
                    .expires_at_unix_ms
                    .saturating_sub(self.confirmed_at_unix_ms)
                    <= MAX_PRIVATE_FILE_GRANT_TTL_MS,
            "private file grant is unconfirmed, expired, or too long-lived"
        );
        anyhow::ensure!(
            !self.files.is_empty() && self.files.len() <= MAX_PRIVATE_FILE_GRANT_FILES,
            "private file grant file count is invalid"
        );
        let mut previous: Option<&str> = None;
        let mut total_bytes = 0_u64;
        let mut folded_paths = std::collections::HashSet::new();
        for file in &self.files {
            validate_private_path(&file.path)?;
            if let Some(previous) = previous {
                anyhow::ensure!(
                    previous < file.path.as_str(),
                    "private file grant paths are duplicated or unsorted"
                );
            }
            previous = Some(&file.path);
            anyhow::ensure!(
                folded_paths.insert(file.path.to_lowercase()),
                "private file grant paths collide on a case-insensitive filesystem"
            );
            anyhow::ensure!(
                file.size_bytes <= MAX_PRIVATE_FILE_BYTES && is_sha256(&file.sha256),
                "private file grant contains invalid file metadata"
            );
            total_bytes = total_bytes.saturating_add(file.size_bytes);
        }
        anyhow::ensure!(
            total_bytes <= MAX_PRIVATE_FILE_GRANT_FILES as u64 * MAX_PRIVATE_FILE_BYTES,
            "private file grant exceeds its total size limit"
        );
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= 64 * 1024,
            "private file grant receipt is oversized"
        );
        Ok(())
    }

    /// The selected path must already be excluded by the normal workspace
    /// snapshot. This keeps private files outside workspace/result manifests.
    pub fn validate_against_exclusions(&self, excluded_paths: &[String]) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.files.iter().all(|file| {
                excluded_paths.iter().any(|excluded| {
                    file.path == *excluded
                        || file
                            .path
                            .strip_prefix(excluded)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                })
            }),
            "private file grant includes a path absent from the workspace exclusion set"
        );
        Ok(())
    }

    pub fn validate_complete_file(&self, path: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let spec = self
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or_else(|| anyhow::anyhow!("private file is outside its grant"))?;
        anyhow::ensure!(
            bytes.len() as u64 == spec.size_bytes && digest(bytes) == spec.sha256,
            "private file content does not match its authorized size and digest"
        );
        Ok(())
    }
}

/// Receipt created only by the explicit operator-confirmation action. Its
/// private fields prevent callers from persisting a grant by setting a boolean
/// or editing serialized draft fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserConfirmedPrivateFileGrantV1 {
    pub(crate) grant: RemotePrivateFileGrantV1,
}

impl UserConfirmedPrivateFileGrantV1 {
    pub(crate) fn after_confirmation(
        mut grant: RemotePrivateFileGrantV1,
        operator_id: String,
        confirmed_at_unix_ms: u64,
    ) -> anyhow::Result<Self> {
        grant.operator_id = operator_id;
        grant.confirmed_at_unix_ms = confirmed_at_unix_ms;
        grant.validate_at(confirmed_at_unix_ms)?;
        Ok(Self { grant })
    }
}

pub(super) fn validate_private_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(path.len() <= 4096, "private file path is too long");
    anyhow::ensure!(
        !path
            .split('/')
            .any(|component| component.eq_ignore_ascii_case(".git")),
        "Git metadata cannot be included in a private file grant"
    );
    crate::execution::remote::workspace::validate_private_excluded_path(path)
}

fn canonical_uuid(value: &str) -> anyhow::Result<()> {
    let parsed = uuid::Uuid::parse_str(value)?;
    anyhow::ensure!(parsed.hyphenated().to_string() == value);
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "private_files/tests.rs"]
mod tests;
