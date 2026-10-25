use std::{
    fs::File,
    os::{fd::AsRawFd, unix::ffi::OsStringExt},
};

pub(super) fn read_child_names(directory: &File) -> anyhow::Result<Vec<std::ffi::OsString>> {
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    anyhow::ensure!(duplicate >= 0, "cache directory could not be duplicated");
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe { libc::close(duplicate) };
        anyhow::bail!("cache directory could not be read");
    }
    let result = (|| {
        let mut names = Vec::new();
        loop {
            set_errno(0);
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                anyhow::ensure!(get_errno() == 0, "cache directory could not be read");
                break;
            }
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                names.push(std::ffi::OsString::from_vec(name.to_vec()));
            }
        }
        Ok(names)
    })();
    unsafe { libc::closedir(stream) };
    result
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn errno_pointer() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(any(
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn errno_pointer() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

fn set_errno(value: libc::c_int) {
    unsafe { *errno_pointer() = value };
}

fn get_errno() -> libc::c_int {
    unsafe { *errno_pointer() }
}
