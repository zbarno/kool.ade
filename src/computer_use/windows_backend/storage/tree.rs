//! Lock down existing files and directories below the app-owned storage root.

use std::{
    io,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
    Security::{EqualSid, OWNER_SECURITY_INFORMATION},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    },
};

use super::security::{
    preexisting_acl_is_safe_to_lock, private_acl_matches_current_user_and_system, set_private_dacl,
};

const MAX_STORAGE_DEPTH: usize = 64;
const MAX_STORAGE_OBJECTS: usize = 100_000;

/// Require safe ACLs on existing contents before the tree is used, then
/// normalize them. Broad legacy ACLs, reparse points, and foreign owners fail closed.
pub(super) fn lock_existing_descendants(root: &Path, current_sid: &[u8]) -> io::Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut inspected = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            inspected = inspected.saturating_add(1);
            if inspected > MAX_STORAGE_OBJECTS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Windows VM storage contains too many objects",
                ));
            }
            if lock_object(&path, current_sid)? {
                if depth >= MAX_STORAGE_DEPTH {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Windows VM storage nesting is too deep",
                    ));
                }
                pending.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

fn lock_object(path: &Path, current_sid: &[u8]) -> io::Result<bool> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // OPEN_REPARSE_POINT makes the attribute check inspect the link itself.
    // SAFETY: path is NUL-terminated and the requested rights are explicit.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = lock_handle(handle, current_sid);
    // SAFETY: this function owns the handle returned by CreateFileW.
    unsafe { CloseHandle(handle) };
    result
}

fn lock_handle(handle: HANDLE, current_sid: &[u8]) -> io::Result<bool> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `info` is a writable output structure.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reparse point found inside Windows VM storage",
        ));
    }
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: output pointers are writable and the handle identifies this object.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    // SAFETY: owner points into the descriptor returned by GetSecurityInfo.
    let owned_by_user =
        !owner.is_null() && unsafe { EqualSid(owner, current_sid.as_ptr().cast_mut().cast()) != 0 };
    // SAFETY: descriptor was allocated by GetSecurityInfo.
    unsafe { LocalFree(descriptor.cast()) };
    if !owned_by_user {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows VM storage object is not owned by the current user",
        ));
    }
    if !preexisting_acl_is_safe_to_lock(handle, current_sid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "existing Windows VM storage object has a non-private ACL",
        ));
    }
    set_private_dacl(handle, current_sid)?;
    if !private_acl_matches_current_user_and_system(handle, current_sid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows VM storage object ACL is not restricted to its owner and SYSTEM",
        ));
    }
    Ok(info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)
}
