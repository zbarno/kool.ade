use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::ComputerBackendErrorV1;

const OWNER_FILE: &str = "owner.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MachineOwnerRecordV1 {
    pub schema_version: u16,
    pub owner_id: String,
    pub project_id: String,
    pub task_id: String,
    pub session_id: String,
    pub machine_id: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub cancellation_generation: u64,
    pub policy_fingerprint: String,
    pub image_digest: String,
    pub image_manifest_digest: String,
    pub image_signer_key_id: String,
    pub catalog_generation: u64,
    pub overlay_path: PathBuf,
    pub state: String,
    pub event_sequence: u64,
    #[serde(default)]
    pub guest_boot_id: Option<String>,
    #[serde(default)]
    pub guest_transcript_sha256: Option<String>,
    #[serde(default)]
    pub guest_ready_sequence: Option<u64>,
    pub pid: Option<u32>,
    pub process_start_ticks: Option<u64>,
    pub exit_code: Option<i32>,
}

pub(super) fn with_registry_lock<T>(
    root: &Path,
    run: impl FnOnce() -> Result<T, ComputerBackendErrorV1>,
) -> Result<T, ComputerBackendErrorV1> {
    let path = root.join(".registry.lock");
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| ComputerBackendErrorV1::Io)?;
    set_private(&file)?;
    file.lock_exclusive()
        .map_err(|_| ComputerBackendErrorV1::Io)?;
    let result = run();
    let _ = FileExt::unlock(&file);
    result
}

pub(super) fn write_owner(
    directory: &Path,
    record: &MachineOwnerRecordV1,
) -> Result<(), ComputerBackendErrorV1> {
    let bytes = serde_json::to_vec(record).map_err(|_| ComputerBackendErrorV1::Io)?;
    let temp = directory.join(format!("owner.json.tmp.{}", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|_| ComputerBackendErrorV1::Io)?;
    set_private(&file)?;
    file.write_all(&bytes)
        .map_err(|_| ComputerBackendErrorV1::Io)?;
    file.sync_all().map_err(|_| ComputerBackendErrorV1::Io)?;
    fs::rename(&temp, directory.join(OWNER_FILE)).map_err(|_| ComputerBackendErrorV1::Io)?;
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ComputerBackendErrorV1::Io)
}

pub(super) fn read_owner(directory: &Path) -> Result<MachineOwnerRecordV1, ComputerBackendErrorV1> {
    let path = directory.join(OWNER_FILE);
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| ComputerBackendErrorV1::OwnershipMismatch)?;
    if !metadata.file_type().is_file() {
        return Err(ComputerBackendErrorV1::OwnershipMismatch);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(ComputerBackendErrorV1::OwnershipMismatch);
        }
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(32 * 1024).read_to_end(&mut bytes))
        .map_err(|_| ComputerBackendErrorV1::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| ComputerBackendErrorV1::OwnershipMismatch)
}

pub(super) fn count_machine_slots(
    root: &Path,
    machine_id: &str,
) -> Result<usize, ComputerBackendErrorV1> {
    let sessions = root.join("sessions");
    fs::create_dir_all(&sessions).map_err(|_| ComputerBackendErrorV1::Io)?;
    set_private_dir(&sessions)?;
    let mut count = 0;
    for entry in fs::read_dir(&sessions).map_err(|_| ComputerBackendErrorV1::Io)? {
        let entry = entry.map_err(|_| ComputerBackendErrorV1::Io)?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| ComputerBackendErrorV1::OwnershipMismatch)?;
        if !metadata.file_type().is_dir() {
            return Err(ComputerBackendErrorV1::OwnershipMismatch);
        }
        let record = read_owner(&entry.path())?;
        if record.schema_version != 1 || matches!(record.state.as_str(), "released" | "stopped") {
            continue;
        }
        if record.machine_id == machine_id {
            count += 1;
        }
    }
    Ok(count)
}

pub(super) fn set_private(file: &File) -> Result<(), ComputerBackendErrorV1> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ComputerBackendErrorV1::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Err(ComputerBackendErrorV1::UnsupportedHost)
    }
}

pub(super) fn set_private_dir(path: &Path) -> Result<(), ComputerBackendErrorV1> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| ComputerBackendErrorV1::UnsafeRuntimeDirectory)?;
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(ComputerBackendErrorV1::UnsafeRuntimeDirectory);
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| ComputerBackendErrorV1::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ComputerBackendErrorV1::UnsupportedHost)
    }
}
