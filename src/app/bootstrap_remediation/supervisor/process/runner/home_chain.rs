use std::{
    ffi::{CStr, CString},
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
};

const DIRECTORY_FLAGS: i32 =
    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;

/// Open a user's home by walking and validating every directory in its path.
/// Writable shared ancestors are accepted only when sticky and owned by root
/// or the target user, as with the conventional root-owned `/tmp` directory.
pub(super) fn open_trusted_home(path: &Path, uid: u32) -> anyhow::Result<File> {
    use std::os::unix::ffi::OsStrExt;

    anyhow::ensure!(
        path.is_absolute(),
        "home directory must be an absolute path"
    );
    anyhow::ensure!(uid != 0, "rootless home cannot belong to root");
    let root = CString::new("/")?;
    let root_fd = unsafe { libc::open(root.as_ptr(), DIRECTORY_FLAGS) };
    anyhow::ensure!(root_fd >= 0, "filesystem root could not be opened safely");
    let mut directory = unsafe { File::from_raw_fd(root_fd) };
    validate_directory(&directory, 0, false)?;

    let components = path
        .as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    anyhow::ensure!(!components.is_empty(), "home cannot be filesystem root");

    for (index, component) in components.iter().enumerate() {
        anyhow::ensure!(
            *component != b"." && *component != b".." && !component.contains(&0),
            "home directory contains an unsafe path component"
        );
        let name = CString::new(*component)?;
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), DIRECTORY_FLAGS) };
        anyhow::ensure!(
            fd >= 0,
            "home directory path contains a missing or unsafe component"
        );
        let child = unsafe { File::from_raw_fd(fd) };
        let is_home = index + 1 == components.len();
        validate_directory(&child, uid, is_home)?;
        validate_entry(&directory, &name, &child)?;
        directory = child;
    }
    Ok(directory)
}

fn validate_directory(directory: &File, uid: u32, is_home: bool) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.metadata()?;
    anyhow::ensure!(metadata.is_dir(), "home path component is not a directory");
    if is_home {
        anyhow::ensure!(
            metadata.uid() == uid
                && metadata.mode() & 0o022 == 0
                && metadata.mode() & 0o700 == 0o700,
            "current user's home directory is not safely owned and writable"
        );
        return Ok(());
    }

    anyhow::ensure!(
        metadata.uid() == 0 || metadata.uid() == uid,
        "home ancestor is owned by an untrusted account"
    );
    let writable_by_others = metadata.mode() & 0o022 != 0;
    let sticky = metadata.mode() & 0o1000 != 0;
    anyhow::ensure!(
        !writable_by_others || sticky,
        "home ancestor is writable by other users without sticky protection"
    );
    Ok(())
}

fn validate_entry(parent: &File, name: &CStr, opened: &File) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    anyhow::ensure!(result == 0, "home path changed during validation");
    let stat = unsafe { stat.assume_init() };
    let metadata = opened.metadata()?;
    anyhow::ensure!(
        stat.st_mode & libc::S_IFMT == libc::S_IFDIR
            && stat.st_uid == metadata.uid()
            && stat.st_dev == metadata.dev() as libc::dev_t
            && stat.st_ino == metadata.ino(),
        "home path component changed during validation"
    );
    Ok(())
}

#[cfg(test)]
#[path = "home_chain/tests.rs"]
mod tests;
