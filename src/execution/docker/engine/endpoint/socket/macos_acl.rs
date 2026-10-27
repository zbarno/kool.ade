use std::{
    ffi::CString,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt},
    },
    path::Path,
};

use super::SocketFilesystemError;

pub(super) fn validate_macos_directory_acl(
    directory: &std::fs::File,
) -> Result<(), SocketFilesystemError> {
    use std::os::fd::AsRawFd;

    validate_macos_acl_fd(directory.as_raw_fd())
}

pub(super) fn validate_macos_acl_fd(fd: libc::c_int) -> Result<(), SocketFilesystemError> {
    const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;
    const ACL_FIRST_ENTRY: libc::c_int = 0;

    // Only ENOENT means there is no extended ACL; unsupported and other failures
    // do not establish that the directory is safe.
    let acl = unsafe { acl_get_fd_np(fd, ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOENT) => Ok(()),
            Some(libc::EACCES) | Some(libc::EPERM) => Err(SocketFilesystemError::Inaccessible),
            _ => Err(SocketFilesystemError::Untrusted),
        };
    }

    // macOS has no `acl_is_trivial_np`; an extended ACL is safe here only when
    // its validated entry list is empty. `acl_get_entry` reports the end of an
    // empty list as EINVAL for ACL_FIRST_ENTRY.
    validate_acl(acl, ACL_FIRST_ENTRY)
}

pub(super) fn validate_macos_socket_acl(
    path: &Path,
    expected_owner: u32,
) -> Result<(), SocketFilesystemError> {
    const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;
    const ACL_FIRST_ENTRY: libc::c_int = 0;

    let before = std::fs::symlink_metadata(path).map_err(super::map_socket_io_error)?;
    if before.file_type().is_symlink()
        || !before.file_type().is_socket()
        || before.uid() != expected_owner
        || before.mode() & 0o077 != 0
    {
        return Err(SocketFilesystemError::Untrusted);
    }

    let c_path =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| SocketFilesystemError::Untrusted)?;
    // `acl_get_link_np` inspects the named object without following a symlink.
    let acl = unsafe { acl_get_link_np(c_path.as_ptr(), ACL_TYPE_EXTENDED) };
    let result = if acl.is_null() {
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(()),
            Some(libc::EACCES) | Some(libc::EPERM) => Err(SocketFilesystemError::Inaccessible),
            _ => Err(SocketFilesystemError::Untrusted),
        }
    } else {
        validate_acl(acl, ACL_FIRST_ENTRY)
    };

    let after = std::fs::symlink_metadata(path).map_err(super::map_socket_io_error)?;
    if after.file_type().is_symlink()
        || !after.file_type().is_socket()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.uid() != expected_owner
        || after.mode() & 0o077 != 0
    {
        return Err(SocketFilesystemError::Untrusted);
    }
    result
}

fn validate_acl(
    acl: *mut libc::c_void,
    first_entry: libc::c_int,
) -> Result<(), SocketFilesystemError> {
    let valid = unsafe { acl_valid(acl) } == 0;
    let mut entry = std::ptr::null_mut();
    let first_entry = if valid {
        unsafe { acl_get_entry(acl, first_entry, &mut entry) }
    } else {
        -2
    };
    let no_entries =
        first_entry == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL);
    let released = unsafe { acl_free(acl) };
    if !valid || released != 0 || !no_entries {
        return Err(SocketFilesystemError::Untrusted);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> *mut libc::c_void;
    fn acl_get_link_np(path: *const libc::c_char, acl_type: libc::c_int) -> *mut libc::c_void;
    fn acl_valid(acl: *mut libc::c_void) -> libc::c_int;
    fn acl_get_entry(
        acl: *mut libc::c_void,
        entry_id: libc::c_int,
        entry: *mut *mut libc::c_void,
    ) -> libc::c_int;
    fn acl_free(value: *mut libc::c_void) -> libc::c_int;
}
