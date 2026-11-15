//! Native Windows fixtures for the narrowly permitted AF_UNIX storage leaf.

use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Networking::WinSock::{
        AF_UNIX, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, WSADATA, WSAStartup, bind,
        closesocket, socket,
    },
    Storage::FileSystem::{
        CreateFileW, CreateSymbolicLinkW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        READ_CONTROL,
    },
};

#[test]
fn storage_accepts_only_the_private_exact_session_af_unix_leaf() {
    let root = ShortStorageRoot::new();
    let session = session_path(root.path());
    let path = session.join("guest-control-v1.sock");
    let socket = BoundUnixSocket::new(&path);
    let sid = current_sid();
    assert!(super::super::tree::is_expected_guest_socket(
        root.path(),
        &path
    ));
    let handle = open_leaf_handle(&path);
    super::super::set_private_dacl(handle, &sid).expect("set private socket ACL");
    super::super::tree::validate_guest_socket_handle(handle, &sid)
        .expect("accept exact user-owned AF_UNIX socket with private ACL");
    unsafe { CloseHandle(handle) };
    drop(socket);
}

#[test]
fn storage_rejects_af_unix_socket_with_wrong_name_or_ancestor() {
    for relative in [
        "sessions/123e4567-e89b-12d3-a456-426614174000/other.sock",
        "sessions/nested/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock",
    ] {
        let root = ShortStorageRoot::new();
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("socket parent")).expect("parents");
        let socket = BoundUnixSocket::new(&path);
        assert!(
            !super::super::tree::is_expected_guest_socket(root.path(), &path),
            "{relative}"
        );
        drop(socket);
    }
}

#[test]
fn socket_validation_rejects_wrong_tag_owner_and_acl() {
    let root = ShortStorageRoot::new();
    let session = session_path(root.path());
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
    let handle = open_leaf_handle(&path);
    assert!(super::super::tree::validate_guest_socket_handle(handle, &sid).is_err());
    unsafe { CloseHandle(handle) };
    std::fs::remove_file(&path).expect("remove symbolic-link fixture");

    // A regular file also fails the exact socket-tag validator.
    std::fs::write(&path, b"not a socket").expect("regular-file fixture");
    let handle = open_leaf_handle(&path);
    assert!(super::super::tree::validate_guest_socket_handle(handle, &sid).is_err());
    unsafe { CloseHandle(handle) };
    std::fs::remove_file(&path).expect("remove regular-file fixture");

    let socket = BoundUnixSocket::new(&path);
    let handle = open_leaf_handle(&path);
    super::super::set_private_dacl(handle, &sid).expect("set private socket ACL");
    let mut wrong_sid = sid.clone();
    let last_sid_byte = wrong_sid.last_mut().expect("non-empty SID");
    *last_sid_byte ^= 0xff;
    assert!(super::super::tree::validate_guest_socket_handle(handle, &wrong_sid).is_err());
    super::super::tree::validate_guest_socket_handle(handle, &sid)
        .expect("accept current user socket owner and private ACL");
    unsafe { CloseHandle(handle) };

    super::set_world_access_dacl(&path);
    let handle = open_leaf_handle(&path);
    assert!(super::super::tree::validate_guest_socket_handle(handle, &sid).is_err());
    unsafe { CloseHandle(handle) };
    drop(socket);
}

fn current_sid() -> Vec<u8> {
    super::super::super::process::current_user_sid()
        .expect("current SID")
        .to_vec()
}

fn session_path(root: &Path) -> PathBuf {
    let session = root
        .join("sessions")
        .join("123e4567-e89b-12d3-a456-426614174000");
    std::fs::create_dir_all(&session).expect("create session directory");
    session
}

struct ShortStorageRoot {
    path: PathBuf,
}

impl ShortStorageRoot {
    fn new() -> Self {
        static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
        let user_profile = std::env::var_os("USERPROFILE").expect("user profile");
        let user_profile = PathBuf::from(user_profile);
        for _ in 0..1024 {
            let name = format!(
                "k{:x}{:x}",
                std::process::id(),
                NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
            );
            let directory = user_profile.join(name);
            match std::fs::create_dir(&directory) {
                Ok(()) => return Self { path: directory },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create short temporary storage root: {error}"),
            }
        }
        panic!("unable to allocate a unique short storage root");
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ShortStorageRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).expect("remove short temporary storage root");
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
