use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use super::super::{
    GuestMessageV1, GuestProtocolErrorV1, types::GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1,
};

const DOWNLOADS: &str = "/home/koolade/Downloads";
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ACTIVE_WRITES: usize = 4;

pub(super) struct PendingWrite {
    file_name: String,
    total_bytes: u64,
    sha256: String,
    next_offset: u64,
}

pub(super) fn write_chunk(
    transfers: &mut HashMap<String, PendingWrite>,
    message: &GuestMessageV1,
) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    let root = downloads_root()?;
    write_chunk_in(transfers, message, &root)
}

fn write_chunk_in(
    transfers: &mut HashMap<String, PendingWrite>,
    message: &GuestMessageV1,
    root: &Path,
) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    let GuestMessageV1::ArtifactWriteChunk {
        transfer_id,
        file_name,
        offset,
        total_bytes,
        sha256,
        bytes,
    } = message
    else {
        return Err(GuestProtocolErrorV1::Malformed);
    };
    validate_transfer(transfer_id, file_name, *total_bytes, bytes.len(), sha256)?;
    if offset
        .checked_add(bytes.len() as u64)
        .is_none_or(|end| end > *total_bytes)
    {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    let state = if let Some(state) = transfers.get_mut(transfer_id) {
        if state.file_name != *file_name
            || state.total_bytes != *total_bytes
            || state.sha256 != *sha256
            || state.next_offset != *offset
        {
            return Err(GuestProtocolErrorV1::Replay);
        }
        state
    } else {
        if *offset != 0 {
            return Err(GuestProtocolErrorV1::SequenceGap);
        }
        if transfers.len() >= MAX_ACTIVE_WRITES {
            return Err(GuestProtocolErrorV1::Oversized);
        }
        let state = PendingWrite {
            file_name: file_name.clone(),
            total_bytes: *total_bytes,
            sha256: sha256.clone(),
            next_offset: 0,
        };
        let temp = temp_path(root, transfer_id);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temp).map_err(|_| GuestProtocolErrorV1::Io)?;
        transfers.insert(transfer_id.clone(), state);
        transfers.get_mut(transfer_id).expect("transfer inserted")
    };
    let temp = temp_path(root, transfer_id);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&temp)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    file.write_all(bytes)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    state.next_offset = state
        .next_offset
        .checked_add(bytes.len() as u64)
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    if state.next_offset > state.total_bytes {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    let complete = state.next_offset == state.total_bytes;
    if complete {
        file.sync_all().map_err(|_| GuestProtocolErrorV1::Io)?;
        drop(file);
        let content = fs::read(&temp).map_err(|_| GuestProtocolErrorV1::Io)?;
        let digest = format!("sha256:{:x}", Sha256::digest(&content));
        if digest != state.sha256 {
            let _ = fs::remove_file(&temp);
            transfers.remove(transfer_id);
            return Err(GuestProtocolErrorV1::Malformed);
        }
        let destination = root.join(file_name);
        fs::hard_link(&temp, &destination).map_err(|_| GuestProtocolErrorV1::Io)?;
        fs::remove_file(&temp).map_err(|_| GuestProtocolErrorV1::Io)?;
        fs::File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| GuestProtocolErrorV1::Io)?;
        transfers.remove(transfer_id);
    }
    Ok(GuestMessageV1::ArtifactWriteAck {
        transfer_id: transfer_id.clone(),
        next_offset: if complete {
            *total_bytes
        } else {
            *offset + bytes.len() as u64
        },
        complete,
    })
}

pub(super) fn abort_write(
    transfers: &mut HashMap<String, PendingWrite>,
    transfer_id: &str,
) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    let root = downloads_root()?;
    abort_write_in(transfers, transfer_id, &root)
}

fn abort_write_in(
    transfers: &mut HashMap<String, PendingWrite>,
    transfer_id: &str,
    root: &Path,
) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    if !canonical_uuid(transfer_id) {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    transfers.remove(transfer_id);
    let path = temp_path(root, transfer_id);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(GuestProtocolErrorV1::Io),
    }
    .map(|()| GuestMessageV1::ArtifactWriteAck {
        transfer_id: transfer_id.to_owned(),
        next_offset: 0,
        complete: false,
    })
}

pub(super) fn cleanup_stale() -> Result<(), GuestProtocolErrorV1> {
    let root = downloads_root()?;
    for entry in fs::read_dir(&root).map_err(|_| GuestProtocolErrorV1::Io)? {
        let entry = entry.map_err(|_| GuestProtocolErrorV1::Io)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(".koolade-transfer-") || !name.ends_with(".partial") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| GuestProtocolErrorV1::Io)?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            fs::remove_file(entry.path()).map_err(|_| GuestProtocolErrorV1::Io)?;
        }
    }
    Ok(())
}

pub(super) fn read_chunk(message: &GuestMessageV1) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    let root = downloads_root()?;
    read_chunk_in(message, &root)
}

fn read_chunk_in(
    message: &GuestMessageV1,
    root: &Path,
) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
    let GuestMessageV1::ArtifactReadChunk {
        transfer_id,
        file_name,
        offset,
        max_bytes,
    } = message
    else {
        return Err(GuestProtocolErrorV1::Malformed);
    };
    if !canonical_uuid(transfer_id)
        || !valid_file_name(file_name)
        || !(1..=GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1 as u32).contains(max_bytes)
    {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    let path = root.join(file_name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| GuestProtocolErrorV1::Io)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_BYTES
    {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    if *offset > metadata.len() {
        return Err(GuestProtocolErrorV1::SequenceGap);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).map_err(|_| GuestProtocolErrorV1::Io)?;
    let mut all = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut all)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if all.len() as u64 != metadata.len() {
        return Err(GuestProtocolErrorV1::Io);
    }
    let digest = format!("sha256:{:x}", Sha256::digest(&all));
    let end = (*offset + u64::from(*max_bytes)).min(metadata.len());
    let bytes = all[*offset as usize..end as usize].to_vec();
    Ok(GuestMessageV1::ArtifactReadResult {
        transfer_id: transfer_id.clone(),
        offset: *offset,
        total_bytes: metadata.len(),
        sha256: digest,
        bytes,
        complete: end == metadata.len(),
    })
}

fn downloads_root() -> Result<std::path::PathBuf, GuestProtocolErrorV1> {
    let path = Path::new(DOWNLOADS);
    fs::create_dir_all(path).map_err(|_| GuestProtocolErrorV1::Io)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| GuestProtocolErrorV1::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| GuestProtocolErrorV1::Io)?;
    }
    Ok(path.to_path_buf())
}

fn temp_path(root: &Path, transfer_id: &str) -> std::path::PathBuf {
    root.join(format!(".koolade-transfer-{transfer_id}.partial"))
}

fn validate_transfer(
    id: &str,
    name: &str,
    total: u64,
    chunk: usize,
    digest: &str,
) -> Result<(), GuestProtocolErrorV1> {
    if !canonical_uuid(id)
        || !valid_file_name(name)
        || total == 0
        || total > MAX_BYTES
        || chunk == 0
        || chunk > GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1
        || digest
            .strip_prefix("sha256:")
            .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    Ok(())
}

fn valid_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('.')
        && !value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !value.chars().any(char::is_control)
        && !matches!(value, "." | "..")
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_transfer_is_offset_fenced_bounded_and_no_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"hello artifact";
        let digest = format!("sha256:{:x}", Sha256::digest(bytes));
        let id = uuid::Uuid::new_v4().to_string();
        let mut transfers = HashMap::new();
        let first = GuestMessageV1::ArtifactWriteChunk {
            transfer_id: id.clone(),
            file_name: "report.txt".into(),
            offset: 0,
            total_bytes: bytes.len() as u64,
            sha256: digest.clone(),
            bytes: bytes[..5].to_vec(),
        };
        assert_eq!(
            write_chunk_in(&mut transfers, &first, root.path()).unwrap(),
            GuestMessageV1::ArtifactWriteAck {
                transfer_id: id.clone(),
                next_offset: 5,
                complete: false,
            }
        );
        let replay = GuestMessageV1::ArtifactWriteChunk {
            transfer_id: id.clone(),
            file_name: "report.txt".into(),
            offset: 0,
            total_bytes: bytes.len() as u64,
            sha256: digest.clone(),
            bytes: bytes[..5].to_vec(),
        };
        assert!(write_chunk_in(&mut transfers, &replay, root.path()).is_err());
        let finish = GuestMessageV1::ArtifactWriteChunk {
            transfer_id: id.clone(),
            file_name: "report.txt".into(),
            offset: 5,
            total_bytes: bytes.len() as u64,
            sha256: digest,
            bytes: bytes[5..].to_vec(),
        };
        assert_eq!(
            write_chunk_in(&mut transfers, &finish, root.path()).unwrap(),
            GuestMessageV1::ArtifactWriteAck {
                transfer_id: id,
                next_offset: bytes.len() as u64,
                complete: true,
            }
        );
        assert_eq!(fs::read(root.path().join("report.txt")).unwrap(), bytes);
        let abort_id = uuid::Uuid::new_v4().to_string();
        let partial = GuestMessageV1::ArtifactWriteChunk {
            transfer_id: abort_id.clone(),
            file_name: "partial.txt".into(),
            offset: 0,
            total_bytes: bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
            bytes: bytes[..3].to_vec(),
        };
        write_chunk_in(&mut transfers, &partial, root.path()).unwrap();
        let partial_path = temp_path(root.path(), &abort_id);
        assert!(partial_path.exists());
        abort_write_in(&mut transfers, &abort_id, root.path()).unwrap();
        assert!(!partial_path.exists());
        let new_id = uuid::Uuid::new_v4().to_string();
        let overwrite = GuestMessageV1::ArtifactWriteChunk {
            transfer_id: new_id,
            file_name: "report.txt".into(),
            offset: 0,
            total_bytes: bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
            bytes: bytes.to_vec(),
        };
        assert!(write_chunk_in(&mut transfers, &overwrite, root.path()).is_err());
    }

    #[test]
    fn read_transfer_rejects_paths_and_enforces_chunk_bound() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("source.txt"), b"source").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let malicious = GuestMessageV1::ArtifactReadChunk {
            transfer_id: id.clone(),
            file_name: "../source.txt".into(),
            offset: 0,
            max_bytes: 32,
        };
        assert!(read_chunk_in(&malicious, root.path()).is_err());
        let request = GuestMessageV1::ArtifactReadChunk {
            transfer_id: id,
            file_name: "source.txt".into(),
            offset: 0,
            max_bytes: GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1 as u32,
        };
        assert_eq!(
            read_chunk_in(&request, root.path()).unwrap(),
            GuestMessageV1::ArtifactReadResult {
                transfer_id: match request {
                    GuestMessageV1::ArtifactReadChunk { transfer_id, .. } => transfer_id,
                    _ => unreachable!(),
                },
                offset: 0,
                total_bytes: 6,
                sha256: format!("sha256:{:x}", Sha256::digest(b"source")),
                bytes: b"source".to_vec(),
                complete: true,
            }
        );
    }
}
