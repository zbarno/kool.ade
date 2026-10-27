use std::{
    ffi::CStr,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
};

use super::{DIRECTORY_FLAGS, UNIT_NAME};

/// Confirm that the only resumable unit is the exact file Kool.ad/e owns.
pub(in crate::app::bootstrap_remediation::supervisor::process::runner) fn validate_existing_current_user_rootless_unit(
    home: &Path,
) -> anyhow::Result<bool> {
    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(
        uid != 0,
        "rootless Docker service cannot be installed as root"
    );
    let mut parent = super::open_home_directory(home, uid)?;
    for name in [c".config", c"systemd", c"user"] {
        let Some(directory) = open_existing_directory(&parent, name, uid)? else {
            return Ok(false);
        };
        parent = directory;
    }
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            UNIT_NAME.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(false);
        }
        return Err(error.into());
    }
    drop(unsafe { File::from_raw_fd(fd) });
    super::existing_unit_file(&parent, uid)?;
    Ok(true)
}

fn open_existing_directory(parent: &File, name: &CStr, uid: u32) -> anyhow::Result<Option<File>> {
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), DIRECTORY_FLAGS) };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error.into());
    }
    let directory = unsafe { File::from_raw_fd(fd) };
    super::verify_directory(&directory, uid)?;
    super::verify_directory_entry(parent, name, &directory, uid)?;
    Ok(Some(directory))
}
