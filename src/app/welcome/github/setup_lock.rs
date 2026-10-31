use std::{fs, io, path::Path};

use fs2::FileExt;

/// Holds a cross-process lock for one managed code checkout destination.
///
/// The hidden sidecar is deliberately retained after unlock so concurrent
/// processes always lock the same inode.
pub(crate) struct SetupLock {
    _file: fs::File,
}

impl SetupLock {
    pub(crate) fn acquire(destination: &Path) -> io::Result<Self> {
        let parent = destination.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent")
        })?;
        let name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination name is invalid")
            })?;
        fs::create_dir_all(parent)?;
        let path = parent.join(format!(".{name}.koolade-setup.lock"));
        let file = open_lock_file(&path)?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_lock_file(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "setup lock sidecar must not be a reparse point",
        ));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open_lock_file(_path: &Path) -> io::Result<fs::File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "managed clone setup locks are not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_lock_is_exclusive_between_file_handles() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("kool.ade");
        let first = SetupLock::acquire(&destination).unwrap();
        let second = open_lock_file(&root.path().join(".kool.ade.koolade-setup.lock")).unwrap();

        let error = second.try_lock_exclusive().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        drop(first);
        second.try_lock_exclusive().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn setup_lock_refuses_a_symlink_sidecar() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("kool.ade");
        let target = root.path().join("target");
        fs::write(&target, b"preserve").unwrap();
        symlink(&target, root.path().join(".kool.ade.koolade-setup.lock")).unwrap();

        let result = SetupLock::acquire(&destination);
        assert!(matches!(
            result,
            Err(error) if error.raw_os_error() == Some(libc::ELOOP)
        ));
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
