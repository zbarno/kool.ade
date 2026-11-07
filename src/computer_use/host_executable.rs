use std::{io, path::Path};

pub(crate) fn validate_trusted_system_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let canonical = path.canonicalize()?;
    let metadata = std::fs::symlink_metadata(path)?;
    if canonical != path
        || !metadata.file_type().is_file()
        || metadata.uid() != 0
        || !safe_executable_mode(metadata.permissions().mode())
        || file_capabilities_present(path.as_os_str().as_bytes())?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "system executable ownership or permissions are unsafe",
        ));
    }
    for ancestor in [Path::new("/"), Path::new("/usr"), Path::new("/usr/bin")] {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != 0
            || metadata.permissions().mode() & 0o022 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "system executable ancestor is unsafe",
            ));
        }
    }
    Ok(())
}

fn safe_executable_mode(mode: u32) -> bool {
    mode & 0o022 == 0 && mode & 0o6000 == 0 && mode & 0o111 != 0
}

fn file_capabilities_present(path: &[u8]) -> io::Result<bool> {
    use std::ffi::CString;

    let path = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "executable path is invalid"))?;
    let attribute = c"security.capability";
    // SAFETY: both C strings are NUL terminated; a null value with size zero
    // asks the kernel only for the extended attribute's length.
    let result =
        unsafe { libc::getxattr(path.as_ptr(), attribute.as_ptr(), std::ptr::null_mut(), 0) };
    if result >= 0 {
        capabilities_from_xattr_result(Ok(result))
    } else {
        capabilities_from_xattr_result(Err(io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO)))
    }
}

fn capabilities_from_xattr_result(result: Result<isize, i32>) -> io::Result<bool> {
    match result {
        Ok(_) => Ok(true),
        Err(libc::ENODATA) => Ok(false),
        Err(error) => Err(io::Error::from_raw_os_error(error)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn setuid_setgid_or_writable_executable_modes_are_rejected() {
        assert!(!super::safe_executable_mode(0o4755));
        assert!(!super::safe_executable_mode(0o2755));
        assert!(!super::safe_executable_mode(0o4777));
        assert!(super::safe_executable_mode(0o755));
        assert!(!super::safe_executable_mode(0o644));
    }

    #[test]
    fn file_capabilities_are_rejected_and_unreadable_attributes_fail_closed() {
        assert!(super::capabilities_from_xattr_result(Ok(0)).unwrap());
        assert!(super::capabilities_from_xattr_result(Ok(24)).unwrap());
        assert!(!super::capabilities_from_xattr_result(Err(libc::ENODATA)).unwrap());
        assert!(super::capabilities_from_xattr_result(Err(libc::EACCES)).is_err());
        assert!(super::capabilities_from_xattr_result(Err(libc::EOPNOTSUPP)).is_err());
    }
}
