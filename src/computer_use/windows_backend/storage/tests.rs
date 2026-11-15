use super::*;
use std::{
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
        SetSecurityInfo,
    },
    Security::{
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        READ_CONTROL, WRITE_DAC,
    },
};

/// Boundary: creates private directories in a disposable profile fixture
/// and validates owner, ACL application and non-reparse handles together.
/// A pure test cannot exercise kernel ACL inheritance or reparse semantics.
/// Dependency: native Windows filesystem/security APIs; setup under 1s.
#[test]
fn app_owned_storage_uses_a_protected_per_user_directory_tree() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = WindowsVmStorage::open_at(temporary.path()).expect("secure storage");
    assert!(storage.root().starts_with(temporary.path()));
    assert!(storage.overlay_root().is_dir());
    let session = storage
        .session_directory("123e4567-e89b-12d3-a456-426614174000")
        .expect("isolated session directory");
    assert!(session.is_dir());
    let sid = super::super::process::current_user_sid().expect("current SID");
    for directory in [
        storage.root(),
        &storage.sessions,
        storage.overlay_root(),
        &session,
    ] {
        assert!(private_acl(directory, &sid), "private ACL on {directory:?}");
    }
    let inherited_file = session.join("state.bin");
    std::fs::write(&inherited_file, b"private fixture").expect("inherited private ACL");
    let reopened = WindowsVmStorage::open_at(temporary.path())
        .expect("accept files inheriting the private owner and SYSTEM ACL");
    assert!(private_acl(&inherited_file, &sid));
    assert!(reopened.root().exists());
    assert!(
        storage
            .session_directory("../123e4567-e89b-12d3-a456-426614174000")
            .is_err()
    );
}

#[test]
fn known_folder_lookup_returns_an_absolute_local_app_data_path() {
    let path = super::path::local_app_data().expect("Local AppData known folder");
    assert!(path.is_absolute());
}

#[test]
fn directory_chain_rejects_a_parent_writable_by_everyone() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let shared = temporary.path().join("shared");
    std::fs::create_dir(&shared).expect("shared fixture directory");
    set_world_access_dacl(&shared);
    let sid = super::super::process::current_user_sid().expect("current SID");
    assert!(super::path::validate_directory_chain(&shared, &sid).is_err());
}

#[test]
fn opening_existing_storage_rejects_world_access_descendant_acls() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = WindowsVmStorage::open_at(temporary.path()).expect("secure storage");
    let legacy_file = storage.sessions.join("legacy/state.bin");
    std::fs::create_dir_all(legacy_file.parent().expect("parent"))
        .expect("legacy session directory");
    std::fs::write(&legacy_file, b"fixture").expect("legacy session file");
    set_world_access_dacl(&legacy_file);

    assert!(WindowsVmStorage::open_at(temporary.path()).is_err());
    let sid = super::super::process::current_user_sid().expect("current SID");
    assert!(!private_acl(&legacy_file, &sid));
}

#[path = "af_unix_tests.rs"]
mod af_unix_tests;

fn private_acl(path: &Path, sid: &[u8]) -> bool {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: path exists and is NUL-terminated; this is a read-only ACL handle.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return false;
    }
    let safe = private_acl_matches_current_user_and_system(handle as HANDLE, sid).unwrap_or(false);
    // SAFETY: this helper owns the file handle.
    unsafe { CloseHandle(handle) };
    safe
}

fn set_world_access_dacl(path: &Path) {
    let sddl = "D:P(A;OICI;FA;;;WD)"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor = null_mut();
    // SAFETY: sddl is NUL-terminated and descriptor is a writable output pointer.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        },
        0
    );
    let mut present = 0;
    let mut dacl = null_mut();
    let mut defaulted = 0;
    // SAFETY: descriptor is valid and output pointers are writable.
    assert_ne!(
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) },
        0
    );
    assert_ne!(present, 0);
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: path exists; handle is opened for DACL changes on the file itself.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    assert!(!handle.is_null() && handle != INVALID_HANDLE_VALUE);
    // SAFETY: the descriptor and handle remain live for this ACL replacement.
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null_mut(),
            )
        },
        0
    );
    // SAFETY: both handles/resources are owned by this helper.
    unsafe {
        CloseHandle(handle);
        windows_sys::Win32::Foundation::LocalFree(descriptor.cast());
    }
}
