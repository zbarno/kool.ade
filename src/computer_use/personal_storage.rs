//! Opt-in per-user encrypted personal VM profile primitives.
//!
//! The controller exposes owner-scoped inventory and deletion. Creating or
//! unlocking a persistent session remains gated until the R3 human-login and
//! security acceptance checks are enabled together.

use std::{
    fs::{self, File},
    path::PathBuf,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub(super) const KEY_SERVICE: &str = "com.koolade.personal-vm.v1";
mod key_service;
mod lifecycle;
mod private;
mod records;
#[cfg(test)]
mod tests;
use key_service::{PersonalVmKeyServiceV1, SecretToolKeyServiceV1};
use private::validate_private_directory;

#[derive(Debug, PartialEq, Eq)]
pub enum PersonalVmStorageErrorV1 {
    InvalidIdentity,
    UnsafeDirectory,
    AlreadyExists,
    ProfileLocked,
    LockUnavailable,
    NotFound,
    WrongOwner,
    KeyServiceUnavailable,
    InvalidKey,
    Io,
    InvalidRecord,
    DiskCreationFailed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PersonalVmProfileV1 {
    pub schema_version: u16,
    pub profile_id: String,
    pub owner_id: String,
    pub created_at_unix_ms: u64,
    pub retention_until_unix_ms: u64,
    pub disk_file_name: String,
    pub image_digest: String,
    pub image_manifest_digest: String,
    pub image_signer_key_id: String,
    pub catalog_generation: u64,
    pub virtual_size_bytes: u64,
}

/// In-memory key wrapper. The key is cleared when the last wrapper is dropped.
pub struct PersonalVmDiskKeyV1(pub(super) Zeroizing<[u8; 32]>);

impl std::fmt::Debug for PersonalVmDiskKeyV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PersonalVmDiskKeyV1([REDACTED])")
    }
}

impl PersonalVmDiskKeyV1 {
    #[cfg(test)]
    pub(crate) fn for_test(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn expose_for_qemu_secret_fd(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn as_qemu_secret_fd(&self) -> Result<File, PersonalVmStorageErrorV1> {
        use std::{
            ffi::CString,
            io::Write,
            os::fd::{AsRawFd, FromRawFd},
        };
        let name = CString::new("koolade-personal-vm-key").unwrap();
        // SAFETY: the memfd name is NUL terminated and flags are supported by Linux.
        let raw = unsafe {
            libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
        };
        if raw < 0 {
            return Err(PersonalVmStorageErrorV1::KeyServiceUnavailable);
        }
        // SAFETY: memfd_create returned a new owned descriptor.
        let mut file = unsafe { File::from_raw_fd(raw) };
        file.write_all(self.expose_for_qemu_secret_fd())
            .map_err(|_| PersonalVmStorageErrorV1::Io)?;
        use std::io::Seek;
        file.rewind().map_err(|_| PersonalVmStorageErrorV1::Io)?;
        // Seal the key bytes against mutation before allowing a child process to read them.
        // SAFETY: fcntl is called with a valid memfd descriptor and the required seal mask.
        if unsafe {
            libc::fcntl(
                file.as_raw_fd(),
                libc::F_ADD_SEALS,
                libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE,
            )
        } < 0
        {
            return Err(PersonalVmStorageErrorV1::KeyServiceUnavailable);
        }
        Ok(file)
    }
}

/// Profile metadata and per-user Secret Service key lookup.
///
/// `secret-tool` sends secret material over stdin/stdout to the user's Secret
/// Service. Keys never appear in command arguments, environment variables, or
/// profile records. Missing or locked Secret Service always fails closed.
pub struct PersonalVmProfileStoreV1 {
    pub(super) root: PathBuf,
    key_service: Arc<dyn PersonalVmKeyServiceV1>,
}

pub(crate) struct PersonalVmCreateRequestV1<'a> {
    pub profile_id: &'a str,
    pub session_id: &'a str,
    pub now_unix_ms: u64,
    pub retention_until_unix_ms: u64,
    pub image: &'a crate::computer_use::guest_image::CachedGuestImageV1,
    pub qemu_tool: &'a crate::computer_use::guest_image::QemuImageToolV1,
}

impl PersonalVmProfileStoreV1 {
    pub fn open(
        root: impl Into<PathBuf>,
        secret_tool: impl Into<PathBuf>,
    ) -> Result<Self, PersonalVmStorageErrorV1> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|_| PersonalVmStorageErrorV1::Io)?;
        validate_private_directory(&root)?;
        let secret_tool = secret_tool.into();
        crate::computer_use::validate_trusted_system_executable(&secret_tool)
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        Ok(Self {
            root,
            key_service: Arc::new(SecretToolKeyServiceV1::new(secret_tool)),
        })
    }
}
