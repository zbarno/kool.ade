//! Per-user Windows VM storage with protected ACLs and reparse-point checks.
//! The security boundary is the Windows account SID: same-account processes
//! share filesystem authority and are outside this storage isolation boundary.
#![allow(dead_code)] // Consumed by the Windows WHPX lifecycle adapter integration seam.
#[path = "storage/ancestor_acl.rs"]
mod ancestor_acl;
#[path = "storage/path.rs"]
mod path;
#[path = "storage/security.rs"]
mod security;
#[path = "storage/tree.rs"]
mod tree;
use security::{
    PrivateDirectorySecurity, private_acl_matches_current_user_and_system, set_private_dacl,
};
use std::{
    io,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
    Security::OWNER_SECURITY_INFORMATION,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    },
};

use super::storage_policy::{WindowsStorageFacts, assess_storage_facts, valid_session_id};

const STORAGE_DIRECTORY_PARTS: [&str; 3] = ["Koolade", "ComputerUse", "v1"];

pub(crate) struct WindowsVmStorage {
    root: PathBuf,
    sessions: PathBuf,
    overlays: PathBuf,
}

impl WindowsVmStorage {
    pub(crate) fn open() -> io::Result<Self> {
        Self::open_at(&path::local_app_data()?)
    }

    fn open_at(local_app_data: &Path) -> io::Result<Self> {
        if !local_app_data.is_absolute() {
            return Err(invalid_data("Local AppData is not an absolute path"));
        }
        let current_sid = super::process::current_user_sid()?;
        path::validate_directory_chain(local_app_data, &current_sid)?;

        let mut security = PrivateDirectorySecurity::new(&current_sid)?;
        let root = STORAGE_DIRECTORY_PARTS
            .iter()
            .fold(local_app_data.to_path_buf(), |path, component| {
                path.join(component)
            });
        let sessions = root.join("sessions");
        let overlays = root.join("overlays");
        for path in [&root, &sessions, &overlays] {
            create_or_lock_directory(path, &mut security, &current_sid)?;
        }
        tree::lock_existing_descendants(&root, &current_sid)?;
        Ok(Self {
            root,
            sessions,
            overlays,
        })
    }

    pub(crate) fn session_directory(&self, session_id: &str) -> io::Result<PathBuf> {
        if !valid_session_id(session_id) {
            return Err(invalid_data("session id is not a canonical UUID"));
        }
        let path = self.sessions.join(session_id);
        let sid = super::process::current_user_sid()?;
        let mut security = PrivateDirectorySecurity::new(&sid)?;
        validate_directory(&self.root, &sid, true, true, true, false)?;
        validate_directory(&self.sessions, &sid, true, true, true, false)?;
        create_or_lock_directory(&path, &mut security, &sid)?;
        tree::lock_existing_descendants(&path, &sid)?;
        Ok(path)
    }

    #[allow(dead_code)] // Consumed by the Windows VM lifecycle adapter.
    pub(crate) fn overlay_root(&self) -> &Path {
        &self.overlays
    }

    #[allow(dead_code)] // Consumed by the Windows VM lifecycle adapter.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

fn create_or_lock_directory(
    path: &Path,
    security: &mut PrivateDirectorySecurity,
    current_sid: &[u8],
) -> io::Result<()> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: the path is NUL-terminated and the descriptor outlives this call.
    let created = unsafe { CreateDirectoryW(wide.as_ptr(), &security.attributes) };
    if created == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error);
        }
    }
    validate_directory(path, current_sid, true, true, created == 0, false)
}

fn validate_directory(
    path: &Path,
    current_sid: &[u8],
    lock_acl: bool,
    require_owner: bool,
    require_private_acl: bool,
    require_parent_acl: bool,
) -> io::Result<()> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // OPEN_REPARSE_POINT inspects the link itself instead of following it.
    // SAFETY: the path is NUL-terminated and all requested access is read/control access.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL | if lock_acl { WRITE_DAC } else { 0 },
            FILE_SHARE_READ
                | FILE_SHARE_WRITE
                | if require_parent_acl {
                    0
                } else {
                    FILE_SHARE_DELETE
                },
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = validate_directory_handle(
        handle,
        current_sid,
        lock_acl,
        require_owner,
        require_private_acl,
        require_parent_acl,
    );
    // SAFETY: `handle` is owned and remains valid through validation.
    unsafe { CloseHandle(handle) };
    result
}

fn validate_directory_handle(
    handle: HANDLE,
    current_sid: &[u8],
    lock_acl: bool,
    require_owner: bool,
    require_private_acl: bool,
    require_parent_acl: bool,
) -> io::Result<()> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `info` is a writable structure as required by this API.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: output pointers are writable and the handle is an open file object.
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
    let current_user_owner = if require_owner && !owner.is_null() {
        // SAFETY: `owner` points inside the live descriptor returned by GetSecurityInfo.
        unsafe {
            windows_sys::Win32::Security::EqualSid(owner, current_sid.as_ptr().cast_mut().cast())
                != 0
        }
    } else {
        true
    };
    // SAFETY: `descriptor` is the security descriptor allocated by GetSecurityInfo.
    unsafe { LocalFree(descriptor.cast()) };
    let facts = WindowsStorageFacts {
        absolute: true,
        directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        reparse_point: info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        current_user_owner,
        // Check shape and ownership before changing any security metadata.
        private_acl: true,
    };
    assess_storage_facts(facts).map_err(|blocker| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("unsafe Windows VM directory: {blocker:?}"),
        )
    })?;
    if require_parent_acl && !ancestor_acl::directory_parent_is_safe(handle, current_sid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows VM storage ancestor is writable by an untrusted account",
        ));
    }
    if require_private_acl && !private_acl_matches_current_user_and_system(handle, current_sid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "existing Windows VM directory has a non-private ACL",
        ));
    }
    if lock_acl {
        set_private_dacl(handle, current_sid)?;
        if !private_acl_matches_current_user_and_system(handle, current_sid)? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows VM directory ACL is not restricted to its owner and SYSTEM",
            ));
        }
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, target_os = "windows"))]
#[path = "storage/tests.rs"]
mod tests;
