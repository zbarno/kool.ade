//! Resolve and validate the Windows per-user storage base path.

use std::{
    ffi::OsString,
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Storage::FileSystem::GetDriveTypeW,
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

use super::super::storage_policy::is_fixed_local_volume;

pub(super) fn local_app_data() -> io::Result<PathBuf> {
    // SHGetKnownFolderPath uses COM. A changed apartment means COM is already
    // initialized on this thread, so do not balance that caller-owned setup.
    let com_status = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    let uninitialize_com = com_status >= 0;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;
    if com_status < 0 && com_status != RPC_E_CHANGED_MODE {
        return Err(io::Error::from_raw_os_error(com_status));
    }
    let mut path = null_mut();
    // SAFETY: `path` is an output pointer and the known-folder ID is static.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DEFAULT as u32,
            null_mut(),
            &mut path,
        )
    };
    if result < 0 || path.is_null() {
        if !path.is_null() {
            // SAFETY: SHGetKnownFolderPath allocates output with the COM task allocator.
            unsafe { CoTaskMemFree(path.cast()) };
        }
        if uninitialize_com {
            // SAFETY: this balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        return Err(io::Error::other("Local AppData lookup failed"));
    }
    // SAFETY: the successful shell call returned a NUL-terminated UTF-16 path.
    let wide = unsafe {
        let mut length = 0;
        while *path.add(length) != 0 {
            length += 1;
        }
        std::slice::from_raw_parts(path, length)
    };
    let local_app_data = PathBuf::from(OsString::from_wide(wide));
    // SAFETY: SHGetKnownFolderPath allocated this buffer with the COM task allocator.
    unsafe { CoTaskMemFree(path.cast()) };
    if uninitialize_com {
        // SAFETY: this balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    Ok(local_app_data)
}

pub(super) fn validate_directory_chain(path: &Path, current_sid: &[u8]) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) if matches!(prefix.kind(), Prefix::Disk(_)) => {
                current.push(component.as_os_str());
            }
            Component::Prefix(_) => {
                return Err(invalid_data("Local AppData must use a local drive path"));
            }
            Component::RootDir => {
                current.push(component.as_os_str());
                let root = current
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                // DRIVE_FIXED is 3. Mapped network drives and removable media
                // are not suitable for per-user VM state.
                if !is_fixed_local_volume(unsafe { GetDriveTypeW(root.as_ptr()) }) {
                    return Err(invalid_data(
                        "Local AppData must be on a fixed local volume",
                    ));
                }
                super::validate_directory(&current, current_sid, false, false, false, true)?;
            }
            Component::Normal(_) => {
                current.push(component.as_os_str());
                super::validate_directory(
                    &current,
                    current_sid,
                    false,
                    current == path,
                    false,
                    true,
                )?;
            }
            Component::CurDir | Component::ParentDir => {
                return Err(invalid_data("Local AppData contains a non-canonical path"));
            }
        }
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
