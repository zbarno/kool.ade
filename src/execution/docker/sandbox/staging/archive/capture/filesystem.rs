use std::{fs, path::Component, path::Path};

#[cfg(unix)]
pub(super) fn open_root_nofollow(path: &Path) -> anyhow::Result<fs::File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let mut current = fs::File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = std::ffi::CString::new(name.as_bytes())?;
                let fd = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
                    )
                };
                anyhow::ensure!(
                    fd >= 0,
                    "workspace root contains a symlink or unavailable directory"
                );
                current = unsafe { fs::File::from_raw_fd(fd) };
            }
            _ => anyhow::bail!("workspace root is not canonical"),
        }
    }
    Ok(current)
}

#[cfg(unix)]
pub(super) fn open_directory_nofollow(parent: &fs::File, path: &Path) -> anyhow::Result<fs::File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let mut current = parent.try_clone()?;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = std::ffi::CString::new(name.as_bytes())?;
                let fd = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
                    )
                };
                anyhow::ensure!(
                    fd >= 0,
                    "workspace scope contains a symlink or unavailable directory"
                );
                current = unsafe { fs::File::from_raw_fd(fd) };
            }
            _ => anyhow::bail!("workspace scope is not canonical"),
        }
    }
    Ok(current)
}

#[cfg(unix)]
pub(super) fn read_child_names(directory: &fs::File) -> anyhow::Result<Vec<std::ffi::OsString>> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStringExt};
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    anyhow::ensure!(
        duplicate >= 0,
        "workspace directory could not be duplicated"
    );
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe {
            libc::close(duplicate);
        }
        anyhow::bail!("workspace directory could not be read");
    }
    let mut names = Vec::new();
    loop {
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        names.push(std::ffi::OsString::from_vec(name.to_vec()));
    }
    unsafe {
        libc::closedir(stream);
    }
    Ok(names)
}
