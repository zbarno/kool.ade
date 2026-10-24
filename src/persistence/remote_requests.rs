//! Operator-local durable retry envelopes for remote worker RPC.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

mod api;
pub use api::{deny_pending_start, pending, record_completion, save_pending, take_completion};
#[cfg(test)]
use api::{pending_at, record_completion_at, save_pending_at, take_completion_at};

const MAX_PENDING: usize = 4096;
const MAX_RECORD_BYTES: u64 = 2 * 1024 * 1024;
pub(crate) const REPLAY_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PendingRemoteRequestV1 {
    pub schema_version: u8,
    pub worker_id: String,
    pub created_at_unix_ms: u64,
    pub envelope: crate::execution::remote::protocol::EnvelopeV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompletedRemoteRequestV1 {
    pub schema_version: u8,
    pub worker_id: String,
    pub request_id: String,
    pub completed_at_unix_ms: u64,
    pub response: crate::execution::remote::protocol::EnvelopeV1,
}

fn load_pending_dir(
    directory: &Path,
    worker_id: &str,
) -> anyhow::Result<Vec<PendingRemoteRequestV1>> {
    let mut requests = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("pending") {
            continue;
        }
        let request = read_pending(&path)?;
        anyhow::ensure!(
            request.worker_id == worker_id,
            "pending request worker mismatch"
        );
        requests.push(request);
        anyhow::ensure!(
            requests.len() <= MAX_PENDING,
            "too many pending remote requests"
        );
    }
    requests.sort_by_key(|request| request.created_at_unix_ms);
    Ok(requests)
}

fn read_pending(path: &Path) -> anyhow::Result<PendingRemoteRequestV1> {
    let bytes = read_private(path)?;
    let mut record: PendingRemoteRequestV1 = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        record.schema_version == 1
            && record.created_at_unix_ms > 0
            && record.envelope.request_id == request_id_from_path(path),
        "pending remote request is invalid"
    );
    record.envelope.validate()?;
    if record.envelope.kind == crate::execution::remote::protocol::EnvelopeKindV1::PrivateFileChunk
    {
        record.envelope.clear_sensitive_payload();
        anyhow::bail!("secret-bearing private file chunks cannot be loaded for retry");
    }
    Ok(record)
}

fn read_completed(path: &Path) -> anyhow::Result<CompletedRemoteRequestV1> {
    let bytes = read_private(path)?;
    let mut record: CompletedRemoteRequestV1 = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        record.schema_version == 1
            && record.completed_at_unix_ms > 0
            && record.request_id == request_id_from_path(path)
            && record.response.request_id == record.request_id,
        "completed remote request is invalid"
    );
    record.response.validate()?;
    if record.response.kind == crate::execution::remote::protocol::EnvelopeKindV1::PrivateFileChunk
    {
        record.response.clear_sensitive_payload();
        anyhow::bail!("secret-bearing private file chunks cannot be loaded as completions");
    }
    Ok(record)
}

fn read_private(path: &Path) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    use std::io;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "remote request record is not a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_RECORD_BYTES,
        "remote request record exceeds its limit"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "remote request record exceeds its limit",
        )
        .into());
    }
    Ok(bytes)
}

fn worker_directory(root: &Path, worker_id: &str, create: bool) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(valid_worker_id(worker_id), "worker ID is invalid");
    let base = root.join("remote-requests");
    if create {
        fs::create_dir_all(&base)?;
        ensure_private_directory(&base)?;
    }
    let directory = base.join(worker_id);
    match fs::symlink_metadata(&base) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => anyhow::bail!("remote request root is not trusted"),
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(directory);
        }
        Err(error) => return Err(error.into()),
    }
    if create {
        fs::create_dir_all(&directory)?;
        ensure_private_directory(&directory)?;
    }
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(directory),
        Ok(_) => anyhow::bail!("remote request directory is not trusted"),
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => Ok(directory),
        Err(error) => Err(error.into()),
    }
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "remote request directory is not trusted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn pending_path(directory: &Path, request_id: &str) -> PathBuf {
    directory.join(format!("{request_id}.pending"))
}

fn completed_path(directory: &Path, request_id: &str) -> PathBuf {
    directory.join(format!("{request_id}.completed"))
}

fn request_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn set_private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn valid_worker_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_request_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

#[cfg(test)]
#[path = "remote_requests/tests.rs"]
mod tests;
