#[cfg(target_os = "linux")]
use std::{io, path::Path};

use crate::{
    app::setup_attention::catalog::{EvidenceCode, EvidenceState},
    computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ResourceCapacity {
    pub available_memory_bytes: Option<u64>,
    pub available_storage_bytes: Option<u64>,
    pub filesystem_family: Option<crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1>,
}

impl ResourceCapacity {
    pub fn state(self) -> EvidenceState {
        let below_budget = self
            .available_memory_bytes
            .is_some_and(|bytes| bytes < MINIMUM_DISPOSABLE_VM_BUDGET_V1.memory_bytes)
            || self
                .available_storage_bytes
                .is_some_and(|bytes| bytes < MINIMUM_DISPOSABLE_VM_BUDGET_V1.disk_bytes);
        if below_budget {
            EvidenceState::Missing
        } else if self.available_memory_bytes.is_some() && self.available_storage_bytes.is_some() {
            EvidenceState::Ready
        } else {
            EvidenceState::Unknown
        }
    }

    pub fn code(self) -> EvidenceCode {
        match self.state() {
            EvidenceState::Ready => EvidenceCode::LinuxVmResourcesAvailable,
            EvidenceState::Missing => EvidenceCode::LinuxVmResourcesInsufficient,
            _ => EvidenceCode::LinuxVmResourcesUnknown,
        }
    }

    pub fn shortages(self) -> (bool, bool) {
        (
            self.available_memory_bytes
                .is_some_and(|bytes| bytes < MINIMUM_DISPOSABLE_VM_BUDGET_V1.memory_bytes),
            self.available_storage_bytes
                .is_some_and(|bytes| bytes < MINIMUM_DISPOSABLE_VM_BUDGET_V1.disk_bytes),
        )
    }
}

pub(super) fn probe() -> ResourceCapacity {
    #[cfg(target_os = "linux")]
    {
        let storage = crate::persistence::setup_tasks::computer_use_storage_dir()
            .ok()
            .and_then(|path| storage_capacity_on_parent(&path).ok());
        ResourceCapacity {
            available_memory_bytes: crate::execution::resources::effective_memory_capacity_bytes(),
            available_storage_bytes: storage.map(|capacity| capacity.available_bytes),
            filesystem_family: storage.and_then(|capacity| capacity.filesystem),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        ResourceCapacity::default()
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StorageCapacity {
    available_bytes: u64,
    filesystem: Option<crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1>,
}

#[cfg(target_os = "linux")]
fn storage_capacity_on_parent(path: &Path) -> io::Result<StorageCapacity> {
    use std::os::unix::ffi::OsStrExt;

    let mut candidate = path;
    loop {
        match std::fs::metadata(candidate) {
            Ok(metadata) if metadata.is_dir() => {
                let path =
                    std::ffi::CString::new(candidate.as_os_str().as_bytes()).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "storage probe path is invalid")
                    })?;
                let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
                // SAFETY: `path` is NUL terminated and `stats` points to a writable statvfs.
                if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: statvfs initialized the output structure on success.
                let stats = unsafe { stats.assume_init() };
                let available_bytes = stats
                    .f_bavail
                    .checked_mul(stats.f_frsize)
                    .ok_or_else(|| io::Error::other("available storage byte count overflowed"))?;
                let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::zeroed();
                // SAFETY: `path` is NUL terminated and `filesystem` points to a writable statfs.
                let filesystem =
                    if unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) } == 0 {
                        // SAFETY: statfs initialized its output structure on success.
                        Some(storage_filesystem_family(
                            unsafe { filesystem.assume_init() }.f_type as u64,
                        ))
                    } else {
                        None
                    };
                return Ok(StorageCapacity {
                    available_bytes,
                    filesystem,
                });
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "Computer Use storage location is not a directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                candidate = candidate.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(target_os = "linux")]
fn storage_filesystem_family(
    filesystem_magic: u64,
) -> crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1 {
    use crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1 as Filesystem;
    const EXT_MAGIC: u64 = 0xef53;
    const XFS_MAGIC: u64 = 0x5846_5342;
    const BTRFS_MAGIC: u64 = 0x9123_683e;
    match filesystem_magic {
        EXT_MAGIC => Filesystem::Ext,
        XFS_MAGIC => Filesystem::Xfs,
        BTRFS_MAGIC => Filesystem::Btrfs,
        _ => Filesystem::Other,
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
