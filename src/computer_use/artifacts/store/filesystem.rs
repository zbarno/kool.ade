use super::{ARTIFACTS, BYTES, CLIPBOARD, GRANTS};
use crate::computer_use::artifacts::{ComputerArtifactBridgeErrorV1, validation};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
};

pub(super) fn read_owned_task_file(
    root: &Path,
    relative: &str,
    limit: u64,
) -> Result<Vec<u8>, ComputerArtifactBridgeErrorV1> {
    #[cfg(unix)]
    {
        read_owned_task_file_unix(root, relative, limit)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, relative, limit);
        Err(ComputerArtifactBridgeErrorV1::Denied)
    }
}

#[cfg(unix)]
fn read_owned_task_file_unix(
    root: &Path,
    relative: &str,
    limit: u64,
) -> Result<Vec<u8>, ComputerArtifactBridgeErrorV1> {
    use std::ffi::CString;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };

    if !validation::valid_relative_path(relative) || path_contains_private_component(relative) {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    let parts = Path::new(relative).components().collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    let mut parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&canonical_root)
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    let mut final_file = None;
    for (index, component) in parts.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        };
        let name =
            CString::new(name.as_bytes()).map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
        let final_component = index + 1 == parts.len();
        let flags = if final_component {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        // SAFETY: parent is an open directory, name is one NUL-free component,
        // and O_NOFOLLOW prevents a substituted symlink at every path step.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
        // SAFETY: openat returned a new owned descriptor on success.
        let opened = unsafe { File::from_raw_fd(fd) };
        if final_component {
            final_file = Some(opened);
        } else {
            parent = opened;
        }
    }
    let file = final_file.ok_or(ComputerArtifactBridgeErrorV1::Denied)?;
    let metadata = file
        .metadata()
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() == 0 || metadata.len() > limit
    {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err(ComputerArtifactBridgeErrorV1::Quota);
    }
    Ok(bytes)
}

pub(super) fn create_user_export(
    destination: &Path,
    bytes: &[u8],
) -> Result<(), ComputerArtifactBridgeErrorV1> {
    let parent = destination
        .parent()
        .ok_or(ComputerArtifactBridgeErrorV1::Denied)?;
    let parent = parent
        .canonicalize()
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    let name = destination
        .file_name()
        .ok_or(ComputerArtifactBridgeErrorV1::Denied)?;
    let path = parent.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            let _ = fs::remove_file(&path);
            ComputerArtifactBridgeErrorV1::Storage
        })
}

pub(super) fn ensure_private_store_directory(
    store_root: &Path,
    directory: &Path,
) -> Result<(), ComputerArtifactBridgeErrorV1> {
    let relative = directory
        .strip_prefix(store_root)
        .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
    fs::create_dir_all(directory).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
    secure_private_directory(store_root)?;
    let mut current = store_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        };
        current.push(name);
        secure_private_directory(&current)?;
    }
    Ok(())
}

pub(super) fn secure_private_file(
    store_root: &Path,
    path: &Path,
) -> Result<(), ComputerArtifactBridgeErrorV1> {
    let parent = path.parent().ok_or(ComputerArtifactBridgeErrorV1::Denied)?;
    ensure_private_store_directory(store_root, parent)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        let secured =
            fs::symlink_metadata(path).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        if secured.uid() != unsafe { libc::geteuid() }
            || !secured.is_file()
            || secured.file_type().is_symlink()
            || secured.permissions().mode() & 0o077 != 0
        {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
    }
    Ok(())
}

fn secure_private_directory(path: &Path) -> Result<(), ComputerArtifactBridgeErrorV1> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ComputerArtifactBridgeErrorV1::Denied);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        let secured =
            fs::symlink_metadata(path).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        if !secured.is_dir()
            || secured.file_type().is_symlink()
            || secured.uid() != unsafe { libc::geteuid() }
            || secured.permissions().mode() & 0o077 != 0
        {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
    }
    Ok(())
}

pub(super) fn path_contains_private_component(relative: &str) -> bool {
    relative.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        part == ".git"
            || part == ".ssh"
            || part == ".env"
            || part == "id_rsa"
            || part == "id_ed25519"
            || part.ends_with(".pem")
            || part.ends_with(".key")
            || part.contains("credential")
            || part.contains("secret")
    })
}

pub(super) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub(super) fn grant_path(id: &str) -> String {
    format!("{GRANTS}/{id}.json")
}
pub(super) fn clipboard_path(id: &str) -> String {
    format!("{CLIPBOARD}/{id}.json")
}
pub(super) fn clipboard_used_path(id: &str) -> String {
    format!("{CLIPBOARD}/{id}.used")
}
pub(super) fn transfer_used_path(id: &str) -> String {
    format!("{GRANTS}/{id}.used")
}
pub(super) fn revoked_path(id: &str) -> String {
    format!("{GRANTS}/{id}.revoked")
}
pub(super) fn manifest_path(id: &str) -> String {
    format!("{ARTIFACTS}/{id}.json")
}
pub(super) fn blob_path(id: &str) -> String {
    format!("{BYTES}/{id}.bin")
}
