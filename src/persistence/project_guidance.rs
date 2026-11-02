//! User-approved project guidance lives in private local state, outside Git.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: u8 = 1;
pub(crate) const MAX_GUIDANCE_BYTES: usize = 16 * 1024;
const SOURCE_PATH: &str = "koolade://project-settings/approved-agent-guidance";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ApprovedProjectGuidanceV1 {
    pub schema_version: u8,
    pub project_id: String,
    pub receipt_uid: String,
    pub source_path: String,
    pub content_sha256: String,
    pub content: String,
    pub enabled: bool,
    pub approved_at_unix_ms: u64,
}

impl ApprovedProjectGuidanceV1 {
    fn new(project_id: &str, content: &str, enabled: bool) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !content.trim().is_empty() && content.len() <= MAX_GUIDANCE_BYTES,
            "project guidance must contain 1 to 16384 UTF-8 bytes"
        );
        let receipt = Self {
            schema_version: SCHEMA_VERSION,
            project_id: project_id.to_owned(),
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            source_path: SOURCE_PATH.into(),
            content_sha256: sha256(content.as_bytes()),
            content: content.to_owned(),
            enabled,
            approved_at_unix_ms: now_unix_ms(),
        };
        receipt.validate(project_id)?;
        Ok(receipt)
    }

    fn validate(&self, project_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION && self.project_id == project_id,
            "project guidance receipt identity is invalid"
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.receipt_uid).is_ok()
                && self.source_path == SOURCE_PATH
                && !self.content.trim().is_empty()
                && self.content.len() <= MAX_GUIDANCE_BYTES
                && self.content_sha256 == sha256(self.content.as_bytes())
                && self.approved_at_unix_ms > 0,
            "project guidance approval does not match its source content"
        );
        Ok(())
    }
}

pub(crate) fn load_for_root(root: &Path) -> anyhow::Result<Option<ApprovedProjectGuidanceV1>> {
    Ok(load_receipt_for_root(root)?.filter(|receipt| receipt.enabled))
}

pub(crate) fn load_receipt_for_root(
    root: &Path,
) -> anyhow::Result<Option<ApprovedProjectGuidanceV1>> {
    let Ok(project_id) = super::dependency_authorization::project_id(root) else {
        return Ok(None);
    };
    let path = store_path(&project_id);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 64 * 1024,
        "project guidance approval store is not a bounded regular file"
    );
    verify_private_file(&path)?;
    let bytes = fs::read(path)?;
    let receipt: ApprovedProjectGuidanceV1 = serde_json::from_slice(&bytes)?;
    receipt.validate(&project_id)?;
    Ok(Some(receipt))
}

pub(crate) fn save_for_root(
    root: &Path,
    content: &str,
    enabled: bool,
) -> anyhow::Result<Option<ApprovedProjectGuidanceV1>> {
    let project_id = super::dependency_authorization::project_id(root)?;
    let path = store_path(&project_id);
    if content.trim().is_empty() {
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "project guidance approval store must be a regular file"
                );
                fs::remove_file(path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(None);
    }
    let receipt = ApprovedProjectGuidanceV1::new(&project_id, content, enabled)?;
    write_private(&path, &serde_json::to_vec(&receipt)?)?;
    Ok(Some(receipt))
}

fn store_path(project_id: &str) -> PathBuf {
    super::project_dir(project_id).join("approved-agent-guidance.json")
}

fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().expect("guidance store has a parent");
    fs::create_dir_all(parent)?;
    secure_private_directory(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "project guidance approval store must be a regular file"
        );
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    verify_private_file(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    verify_private_file(path)?;
    Ok(())
}

fn secure_private_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(metadata.is_dir() && !metadata.file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(metadata.uid() == unsafe { libc::geteuid() });
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    super::setup_tasks::verify_private_acl(path)
}

fn verify_private_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(metadata.is_file() && !metadata.file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.permissions().mode() & 0o077 == 0,
            "project guidance approval file is not private"
        );
    }
    super::setup_tasks::verify_private_acl(path)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "project_guidance/tests.rs"]
mod tests;
