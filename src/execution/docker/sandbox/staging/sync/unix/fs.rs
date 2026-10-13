use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::Path,
};

use super::super::super::archive::{Fingerprint, hash};

const NOFOLLOW_DIR: i32 = libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY;

pub(super) fn open_root_nofollow(path: &Path) -> Result<File, String> {
    let mut current = OpenOptions::new()
        .read(true)
        .open("/")
        .map_err(|_| "Filesystem root could not be opened")?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                let name =
                    CString::new(name.as_bytes()).map_err(|_| "Workspace root path is invalid")?;
                let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), NOFOLLOW_DIR) };
                if fd < 0 {
                    return Err(
                        "Task-clone root contains a symlink or unavailable directory".into(),
                    );
                }
                current = unsafe { File::from_raw_fd(fd) };
            }
            _ => return Err("Task-clone root is not canonical".into()),
        }
    }
    Ok(current)
}

pub(super) fn parent(root: &File, relative: &Path) -> Result<Option<(File, CString)>, String> {
    super::validate_relative(relative).map_err(|_| "Workspace path is unsafe")?;
    let mut components = relative.components().peekable();
    let mut current = root
        .try_clone()
        .map_err(|_| "Task-clone root could not be duplicated")?;
    while let Some(component) = components.next() {
        let name = CString::new(component.as_os_str().as_bytes())
            .map_err(|_| "Workspace path contains a NUL")?;
        if components.peek().is_none() {
            return Ok(Some((current, name)));
        }
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), NOFOLLOW_DIR) };
        if fd < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err("Workspace path traverses a non-directory or symlink entry".into());
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Err("Workspace path is empty".into())
}

pub(super) fn fingerprint_at(
    root: &File,
    relative: &Path,
    byte_limit: u64,
) -> Result<Option<Fingerprint>, String> {
    let Some((parent, name)) = parent(root, relative)? else {
        return Ok(None);
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err("Workspace destination could not be inspected safely".into());
    }
    let stat = unsafe { stat.assume_init() };
    let kind = stat.st_mode & libc::S_IFMT;
    if kind == libc::S_IFLNK {
        return Err("Workspace destination is a symlink".into());
    }
    if kind == libc::S_IFDIR {
        return Ok(Some(Fingerprint::Directory));
    }
    if kind != libc::S_IFREG {
        return Err("Workspace destination is a special file".into());
    }
    if stat.st_size < 0 || stat.st_size as u64 > byte_limit {
        return Err("Workspace destination exceeds the synchronization limit".into());
    }
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err("Workspace destination changed or could not be opened safely".into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file
        .metadata()
        .map_err(|_| "Workspace destination metadata could not be read")?;
    if !metadata.is_file() {
        return Err("Workspace destination changed during inspection".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "Workspace destination could not be read")?;
    if bytes.len() as u64 > byte_limit {
        return Err("Workspace destination exceeds the synchronization limit".into());
    }
    let after = file
        .metadata()
        .map_err(|_| "Workspace destination metadata could not be rechecked")?;
    if after.len() != metadata.len()
        || after.mtime() != metadata.mtime()
        || after.mtime_nsec() != metadata.mtime_nsec()
        || after.ctime() != metadata.ctime()
        || after.ctime_nsec() != metadata.ctime_nsec()
    {
        return Err("Workspace destination changed during synchronization".into());
    }
    Ok(Some(Fingerprint::File {
        sha256: hash(&bytes),
        size: bytes.len() as u64,
        mode: metadata.mode() & 0o777,
    }))
}

pub(super) fn create_directory(root: &File, relative: &Path) -> Result<(), String> {
    let (parent, name) =
        parent(root, relative)?.ok_or("Workspace directory parent is unavailable")?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } != 0 {
        return Err("Workspace directory could not be created safely".into());
    }
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), NOFOLLOW_DIR) };
    if fd < 0 {
        return Err("Created workspace directory could not be reopened safely".into());
    }
    let directory = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o755) } != 0
        || unsafe { libc::fsync(parent.as_raw_fd()) } != 0
    {
        return Err("Workspace directory update could not be flushed safely".into());
    }
    Ok(())
}
