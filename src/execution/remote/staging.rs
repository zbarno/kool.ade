//! Durable, bounded file transfer primitives for remote workspace and results.

mod chunk;
mod frame;
mod manifest;
mod receiver;
mod sender;
mod worker;

pub use chunk::{MAX_TRANSFER_CHUNK_BYTES, TransferChunkV1};
pub use frame::{
    MAX_TRANSFER_FRAME_BYTES, TransferFrameV1, decode_transfer_frame, encode_transfer_frame,
    read_transfer_frame, write_transfer_frame,
};
pub use manifest::{TransferManifestV1, TransferPayloadV1};
pub use receiver::{StagingReceiver, VerifiedWorkspaceResult};
pub use sender::stream_file_chunks;
pub use worker::WorkerStager;

/// Returns true when an execution's secret mount was created by an earlier
/// worker process and must be stopped before the stage can be deleted.
pub(crate) enum PrivateFileExecutionRecovery {
    NoMount,
    Active,
    Orphaned { _lock: std::fs::File },
}

/// Classifies a started private-file mount and retains the owner lock through
/// Docker recovery so a second worker cannot reuse the execution ID mid-sweep.
pub(crate) fn lock_private_file_execution_for_recovery(
    state_root: &std::path::Path,
    execution_id: &str,
) -> anyhow::Result<PrivateFileExecutionRecovery> {
    let parsed = uuid::Uuid::parse_str(execution_id)?;
    anyhow::ensure!(parsed.hyphenated().to_string() == execution_id);
    let root = state_root.join("remote-staging").join("private-files");
    let marker = root.join(format!("{execution_id}.started"));
    let metadata = match std::fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PrivateFileExecutionRecovery::NoMount);
        }
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "private-file recovery marker is unsafe"
    );
    anyhow::ensure!(
        std::fs::read(&marker)? == b"private-file-mount-v1\n",
        "private-file recovery marker is invalid"
    );
    let owner = root.join(format!("{execution_id}.owner"));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(owner)?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "private-file owner lock is unsafe"
    );
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => {
            if !marker.exists() {
                return Ok(PrivateFileExecutionRecovery::NoMount);
            }
            Ok(PrivateFileExecutionRecovery::Orphaned { _lock: file })
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Ok(PrivateFileExecutionRecovery::Active)
        }
        Err(error) => Err(error.into()),
    }
}
