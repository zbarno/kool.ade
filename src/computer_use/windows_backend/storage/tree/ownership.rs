//! Ownership adoption for a newly bound Windows AF_UNIX socket leaf.

use std::{
    io,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    Security::{
        Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo,
        GetFileInformationByHandleEx, OPEN_EXISTING, WRITE_OWNER,
    },
    System::SystemServices::IO_REPARSE_TAG_AF_UNIX,
};

use super::is_expected_guest_socket;

/// Winsock uses the token's default owner for AF_UNIX leaves. After bind
/// succeeds, claim the exact new leaf before strict owner and ACL validation.
pub(super) fn set_new_guest_socket_owner(
    storage_root: &Path,
    path: &Path,
    current_sid: &[u8],
) -> io::Result<()> {
    if !is_expected_guest_socket(storage_root, path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "guest-control socket path is not the exact session leaf",
        ));
    }
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // OPEN_REPARSE_POINT ensures ownership applies to the socket leaf itself.
    // SAFETY: path is NUL-terminated and requested access is limited to inspect and owner change.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | WRITE_OWNER,
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
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `tag` is writable and `handle` references the exact leaf itself.
    let is_af_unix_leaf = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        ) != 0
            && tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && tag.ReparseTag == IO_REPARSE_TAG_AF_UNIX
    };
    let result = if !is_af_unix_leaf {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "new guest-control leaf is not a Windows AF_UNIX socket",
        ))
    } else {
        // SAFETY: bind just created this leaf; `current_sid` is a valid user SID
        // and the handle was opened with WRITE_OWNER.
        let status = unsafe {
            SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                current_sid.as_ptr().cast_mut().cast(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    };
    // SAFETY: this function owns the handle returned by CreateFileW.
    unsafe { CloseHandle(handle) };
    result
}
