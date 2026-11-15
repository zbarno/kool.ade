use super::*;
use std::{
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, WSADATA, WSAStartup, bind, closesocket,
    socket,
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
        CreateFileW, CreateSymbolicLinkW, DDD_EXACT_MATCH_ON_REMOVE, DDD_RAW_TARGET_PATH,
        DDD_REMOVE_DEFINITION, DefineDosDeviceW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    },
    System::SystemInformation::GetLogicalDrives,
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

#[test]
fn storage_accepts_only_the_private_exact_session_af_unix_leaf() {
    let short_root = ShortStorageRoot::new();
    let storage = WindowsVmStorage::open_at(short_root.path()).expect("secure storage");
    let session_id = "123e4567-e89b-12d3-a456-426614174000";
    let session = storage.session_directory(session_id).expect("session");
    let socket = BoundUnixSocket::new(&session.join("guest-control-v1.sock"));

    WindowsVmStorage::open_at(short_root.path())
        .expect("accept exact user-owned AF_UNIX socket with inherited private ACL");
    drop(socket);
}

#[test]
fn storage_rejects_af_unix_socket_with_wrong_name_or_ancestor() {
    for relative in [
        "sessions/123e4567-e89b-12d3-a456-426614174000/other.sock",
        "sessions/nested/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock",
    ] {
        let short_root = ShortStorageRoot::new();
        let storage = WindowsVmStorage::open_at(short_root.path()).expect("secure storage");
        let path = storage.root().join(relative);
        std::fs::create_dir_all(path.parent().expect("socket parent")).expect("parents");
        let socket = BoundUnixSocket::new(&path);
        assert!(
            WindowsVmStorage::open_at(short_root.path()).is_err(),
            "{relative}"
        );
        drop(socket);
    }
}

#[test]
fn socket_validation_rejects_wrong_tag_owner_and_acl() {
    let short_root = ShortStorageRoot::new();
    let storage = WindowsVmStorage::open_at(short_root.path()).expect("secure storage");
    let session = storage
        .session_directory("123e4567-e89b-12d3-a456-426614174000")
        .expect("session");
    let path = session.join("guest-control-v1.sock");
    let sid = super::super::process::current_user_sid().expect("current SID");

    // A symbolic link at the permitted name has the wrong reparse tag.
    let target = session.join("target.bin");
    std::fs::write(&target, b"not a socket").expect("symlink target");
    let wide_link = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let wide_target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    assert_ne!(
        unsafe { CreateSymbolicLinkW(wide_link.as_ptr(), wide_target.as_ptr(), 2) },
        0,
        "create unprivileged file symlink fixture"
    );
    assert!(WindowsVmStorage::open_at(short_root.path()).is_err());
    std::fs::remove_file(&path).expect("remove symbolic-link fixture");

    // A regular file also fails the exact socket-tag validator.
    std::fs::write(&path, b"not a socket").expect("regular-file fixture");
    let handle = open_leaf_handle(&path);
    assert!(super::tree::validate_guest_socket_handle(handle, &sid).is_err());
    unsafe { CloseHandle(handle) };
    std::fs::remove_file(&path).expect("remove regular-file fixture");

    let socket = BoundUnixSocket::new(&path);
    let handle = open_leaf_handle(&path);
    let mut wrong_sid = sid.clone();
    let last_sid_byte = wrong_sid.last_mut().expect("non-empty SID");
    *last_sid_byte ^= 0xff;
    assert!(super::tree::validate_guest_socket_handle(handle, &wrong_sid).is_err());
    unsafe { CloseHandle(handle) };

    set_world_access_dacl(&path);
    assert!(WindowsVmStorage::open_at(short_root.path()).is_err());
    drop(socket);
}

struct ShortStorageRoot {
    temporary: tempfile::TempDir,
    device: Vec<u16>,
    target: Vec<u16>,
    path: PathBuf,
}

impl ShortStorageRoot {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let logical_drives = unsafe { GetLogicalDrives() };
        let mut mapping = None;
        for letter in (b'X'..=b'Z').rev() {
            let index = u32::from(letter - b'A');
            if logical_drives & (1 << index) != 0 {
                continue;
            }
            let drive = format!("{}:", letter as char);
            let device = drive.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let target = format!(r"\??\{}", temporary.path().display())
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            if unsafe { DefineDosDeviceW(DDD_RAW_TARGET_PATH, device.as_ptr(), target.as_ptr()) }
                != 0
            {
                mapping = Some((drive, device, target));
                break;
            }
        }
        let (drive, device, target) = mapping.expect("temporary DOS drive mapping");
        Self {
            temporary,
            device,
            target,
            path: PathBuf::from(format!("{drive}\\")),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ShortStorageRoot {
    fn drop(&mut self) {
        let _keep_tempdir_alive = &self.temporary;
        unsafe {
            DefineDosDeviceW(
                DDD_RAW_TARGET_PATH | DDD_REMOVE_DEFINITION | DDD_EXACT_MATCH_ON_REMOVE,
                self.device.as_ptr(),
                self.target.as_ptr(),
            );
        }
    }
}

struct BoundUnixSocket(SOCKET);

impl BoundUnixSocket {
    fn new(path: &Path) -> Self {
        let mut data = WSADATA::default();
        assert_eq!(unsafe { WSAStartup(0x0202, &mut data) }, 0, "WSAStartup");
        let socket = unsafe { socket(AF_UNIX.into(), SOCK_STREAM, 0) };
        assert_ne!(
            socket,
            windows_sys::Win32::Networking::WinSock::INVALID_SOCKET
        );
        let path_bytes = path.to_str().expect("UTF-8 test path").as_bytes();
        assert!(path_bytes.len() < 108, "AF_UNIX path limit");
        let mut address = SOCKADDR_UN::default();
        address.sun_family = AF_UNIX;
        for (slot, byte) in address.sun_path.iter_mut().zip(path_bytes) {
            *slot = *byte as i8;
        }
        assert_eq!(
            unsafe {
                bind(
                    socket,
                    (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
                    std::mem::size_of::<SOCKADDR_UN>() as i32,
                )
            },
            0,
            "bind AF_UNIX fixture"
        );
        Self(socket)
    }
}

impl Drop for BoundUnixSocket {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.0);
            windows_sys::Win32::Networking::WinSock::WSACleanup();
        }
    }
}

fn open_leaf_handle(path: &Path) -> HANDLE {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
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
    assert!(!handle.is_null() && handle != INVALID_HANDLE_VALUE);
    handle
}

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
