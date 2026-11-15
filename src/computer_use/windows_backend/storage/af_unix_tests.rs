//! Native Windows fixtures for the narrowly permitted AF_UNIX storage leaf.

use super::super::WindowsVmStorage;
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Networking::WinSock::{
        AF_UNIX, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, WSADATA, WSAStartup, bind,
        closesocket, socket,
    },
    Storage::FileSystem::GetLogicalDrives,
    Storage::FileSystem::{
        CreateFileW, CreateSymbolicLinkW, DDD_EXACT_MATCH_ON_REMOVE, DDD_RAW_TARGET_PATH,
        DDD_REMOVE_DEFINITION, DefineDosDeviceW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL,
    },
};

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
    let sid = super::super::super::process::current_user_sid()
        .expect("current SID")
        .to_vec();

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
    assert!(
        unsafe { CreateSymbolicLinkW(wide_link.as_ptr(), wide_target.as_ptr(), 2) },
        "create unprivileged file symlink fixture"
    );
    assert!(WindowsVmStorage::open_at(short_root.path()).is_err());
    std::fs::remove_file(&path).expect("remove symbolic-link fixture");

    // A regular file also fails the exact socket-tag validator.
    std::fs::write(&path, b"not a socket").expect("regular-file fixture");
    let handle = open_leaf_handle(&path);
    assert!(super::super::tree::validate_guest_socket_handle(handle, &sid).is_err());
    unsafe { CloseHandle(handle) };
    std::fs::remove_file(&path).expect("remove regular-file fixture");

    let socket = BoundUnixSocket::new(&path);
    let handle = open_leaf_handle(&path);
    let mut wrong_sid = sid.clone();
    let last_sid_byte = wrong_sid.last_mut().expect("non-empty SID");
    *last_sid_byte ^= 0xff;
    assert!(super::super::tree::validate_guest_socket_handle(handle, &wrong_sid).is_err());
    unsafe { CloseHandle(handle) };

    super::set_world_access_dacl(&path);
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
