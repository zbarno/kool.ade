use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};

use zeroize::Zeroizing;

use super::{PersonalVmDiskKeyV1, PersonalVmStorageErrorV1};

pub(super) fn random_key() -> Result<PersonalVmDiskKeyV1, PersonalVmStorageErrorV1> {
    let mut key = Zeroizing::new([0_u8; 32]);
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut key[..]))
        .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
    Ok(PersonalVmDiskKeyV1(key))
}

pub(super) fn validate_uuid(value: &str) -> Result<(), PersonalVmStorageErrorV1> {
    if uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value) {
        Ok(())
    } else {
        Err(PersonalVmStorageErrorV1::InvalidIdentity)
    }
}

pub(super) fn valid_tagged_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        && value[7..].bytes().all(|byte| !byte.is_ascii_uppercase())
}

pub(super) fn validate_private_directory(path: &Path) -> Result<(), PersonalVmStorageErrorV1> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata =
        fs::symlink_metadata(path).map_err(|_| PersonalVmStorageErrorV1::UnsafeDirectory)?;
    if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PersonalVmStorageErrorV1::UnsafeDirectory);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| PersonalVmStorageErrorV1::Io)
}

pub(super) fn validate_private_file(
    metadata: &std::fs::Metadata,
) -> Result<(), PersonalVmStorageErrorV1> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o077 != 0 {
        return Err(PersonalVmStorageErrorV1::UnsafeDirectory);
    }
    Ok(())
}

pub(super) fn private_create_new(path: &Path) -> Result<File, PersonalVmStorageErrorV1> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            PersonalVmStorageErrorV1::AlreadyExists
        } else {
            PersonalVmStorageErrorV1::Io
        }
    })
}
