//! Local receipt for the approved, no-network rootless image smoke test.

use serde::{Deserialize, Serialize};

const FILE: &str = "linux-rootless-smoke.json";
const SCHEMA_VERSION: u8 = 1;
const MAX_BYTES: u64 = 4096;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RootlessSmokeReceiptV1 {
    pub(crate) schema_version: u8,
    pub(crate) user_id: u32,
    pub(crate) endpoint_fingerprint: String,
    pub(crate) daemon_fingerprint: String,
    pub(crate) daemon_version: String,
    pub(crate) image_reference: String,
    pub(crate) image_digest: String,
    pub(crate) image_platform: String,
    pub(crate) completed_at_unix_ms: u64,
}

impl RootlessSmokeReceiptV1 {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION && self.user_id != 0,
            "unsupported rootless image smoke receipt"
        );
        anyhow::ensure!(
            valid_fingerprint(&self.endpoint_fingerprint)
                && valid_fingerprint(&self.daemon_fingerprint),
            "rootless image smoke receipt has an invalid endpoint identity"
        );
        anyhow::ensure!(
            !self.daemon_version.is_empty()
                && self.daemon_version.len() <= 128
                && !self
                    .daemon_version
                    .bytes()
                    .any(|byte| byte.is_ascii_control())
                && self.image_reference.len() <= 512
                && !self.image_reference.is_empty()
                && !self
                    .image_reference
                    .bytes()
                    .any(|byte| byte.is_ascii_control())
                && self
                    .image_digest
                    .strip_prefix("sha256:")
                    .is_some_and(valid_fingerprint)
                && matches!(self.image_platform.as_str(), "linux/amd64" | "linux/arm64")
                && self.completed_at_unix_ms > 0,
            "rootless image smoke receipt has invalid image evidence"
        );
        Ok(())
    }

    fn matches(
        &self,
        endpoint: &str,
        daemon: &str,
        daemon_version: &str,
        image: &crate::execution::docker::images::SelectedImage,
    ) -> bool {
        self.validate().is_ok()
            && self.user_id == current_user_id()
            && self.endpoint_fingerprint == endpoint
            && self.daemon_fingerprint == daemon
            && self.daemon_version == daemon_version
            && self.image_reference == image.reference
            && self.image_digest == image.digest
            && self.image_platform == image.platform
    }
}

/// Persist evidence only after the approved, digest-locked image smoke succeeds.
#[cfg(target_os = "linux")]
pub(crate) fn record(
    endpoint_fingerprint: &str,
    daemon_fingerprint: &str,
    daemon_version: &str,
    image: &crate::execution::docker::images::SelectedImage,
    completed_at_unix_ms: u64,
) -> anyhow::Result<()> {
    let receipt = RootlessSmokeReceiptV1 {
        schema_version: SCHEMA_VERSION,
        user_id: current_user_id(),
        endpoint_fingerprint: endpoint_fingerprint.to_owned(),
        daemon_fingerprint: daemon_fingerprint.to_owned(),
        daemon_version: daemon_version.to_owned(),
        image_reference: image.reference.clone(),
        image_digest: image.digest.clone(),
        image_platform: image.platform.clone(),
        completed_at_unix_ms,
    };
    receipt.validate()?;
    let path = path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("rootless smoke receipt has no parent directory"))?;
    verify_private_directory(parent)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&receipt)?)?;
    secure_private_file(&path)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn record(
    _endpoint_fingerprint: &str,
    _daemon_fingerprint: &str,
    _daemon_version: &str,
    _image: &crate::execution::docker::images::SelectedImage,
    _completed_at_unix_ms: u64,
) -> anyhow::Result<()> {
    anyhow::bail!("rootless image smoke receipts are Linux-only")
}

/// Match a recorded smoke to the current trusted socket, daemon, and locked image.
pub(crate) fn matches_current(
    endpoint_fingerprint: Option<&str>,
    daemon_fingerprint: Option<&str>,
    daemon_version: Option<&str>,
    image: Option<&crate::execution::docker::images::SelectedImage>,
) -> bool {
    matching_current(
        endpoint_fingerprint,
        daemon_fingerprint,
        daemon_version,
        image,
    )
    .is_some()
}

pub(crate) fn matching_current(
    endpoint_fingerprint: Option<&str>,
    daemon_fingerprint: Option<&str>,
    daemon_version: Option<&str>,
    image: Option<&crate::execution::docker::images::SelectedImage>,
) -> Option<RootlessSmokeReceiptV1> {
    #[cfg(target_os = "linux")]
    {
        let (Some(endpoint), Some(daemon), Some(image)) =
            (endpoint_fingerprint, daemon_fingerprint, image)
        else {
            return None;
        };
        let daemon_version = daemon_version?;
        read()
            .ok()
            .flatten()
            .filter(|receipt| receipt.matches(endpoint, daemon, daemon_version, image))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            endpoint_fingerprint,
            daemon_fingerprint,
            daemon_version,
            image,
        );
        None
    }
}

#[cfg(target_os = "linux")]
fn read() -> anyhow::Result<Option<RootlessSmokeReceiptV1>> {
    use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt};

    let path = path()?;
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    verify_private_file_metadata(&metadata)?;
    anyhow::ensure!(
        metadata.len() <= MAX_BYTES,
        "rootless smoke receipt is oversized"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "rootless smoke receipt is oversized"
    );
    let receipt: RootlessSmokeReceiptV1 = serde_json::from_slice(&bytes)?;
    receipt.validate()?;
    Ok(Some(receipt))
}

#[cfg(target_os = "linux")]
fn path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::persistence::setup_tasks::machine_setup_state_dir()?.join(FILE))
}

#[cfg(target_os = "linux")]
fn verify_private_directory(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == current_user_id()
            && metadata.mode() & 0o077 == 0,
        "rootless smoke receipt directory is not private"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_private_file_metadata(metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == current_user_id()
            && metadata.nlink() == 1
            && metadata.mode() & 0o077 == 0,
        "rootless smoke receipt file is not private"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn secure_private_file(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    verify_private_file_metadata(&std::fs::symlink_metadata(path)?)
}

fn current_user_id() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        1
    }
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "linux_rootless_smoke_receipt/tests.rs"]
mod tests;
