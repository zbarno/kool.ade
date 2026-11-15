//! Native Windows fixtures for the narrowly permitted AF_UNIX storage leaf.

use std::{
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Networking::WinSock::{
        AF_UNIX, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, WSADATA, WSAStartup, bind,
        closesocket, socket,
    },
    Security::{
        ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT, SetSecurityInfo},
        DACL_SECURITY_INFORMATION, GetAclInformation, GetSecurityDescriptorDacl,
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{
        CreateFileW, CreateSymbolicLinkW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    },
};

#[test]
fn storage_accepts_only_the_private_exact_session_af_unix_leaf() {
    let root = ShortStorageRoot::new();
    let session = session_path(root.path());
    let sid = current_sid();
    set_directory_private_acl(&session, &sid);
    let path = session.join("guest-control-v1.sock");
    let socket = BoundUnixSocket::new(&path);
    assert!(super::super::tree::is_expected_guest_socket(
        root.path(),
        &path
    ));
    let short_transport_root = root.path().join("t");
    let transport_socket = short_transport_root
        .join("123e4567-e89b-12d3-a456-426614174000")
        .join("guest-control-v1.sock");
    assert!(super::super::tree::is_expected_guest_socket(
        &short_transport_root,
        &transport_socket
    ));
    let handle = open_leaf_handle(&path);
    set_socket_owner(handle, &sid);
    assert_eq!(
        socket_owner_sid(handle),
        sid_string(&sid),
        "AF_UNIX owner SID"
    );
    assert!(
        super::super::security::preexisting_guest_socket_acl_is_safe(handle, &sid)
            .expect("validate socket ACL"),
        "AF_UNIX ACL: {}",
        socket_acl_summary(handle)
    );
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
    let sid = current_sid();
    set_directory_private_acl(&session, &sid);
    let path = session.join("guest-control-v1.sock");

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
    set_socket_owner(handle, &sid);
    assert_eq!(
        socket_owner_sid(handle),
        sid_string(&sid),
        "AF_UNIX owner SID"
    );
    assert!(
        super::super::security::preexisting_guest_socket_acl_is_safe(handle, &sid)
            .expect("validate socket ACL"),
        "AF_UNIX ACL: {}",
        socket_acl_summary(handle)
    );
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

fn socket_owner_sid(handle: HANDLE) -> String {
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: the output pointers are writable and the handle identifies the socket leaf.
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
    assert_eq!(status, 0, "query AF_UNIX owner SID");
    let result = if owner.is_null() {
        "<null>".to_owned()
    } else {
        sid_string_from_ptr(owner)
    };
    unsafe { LocalFree(descriptor.cast()) };
    result
}

fn socket_acl_summary(handle: HANDLE) -> String {
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return format!("GetSecurityInfo status={status}");
    }
    let mut present = 0;
    let mut acl = null_mut();
    let mut defaulted = 0;
    let valid =
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            != 0;
    let mut info = ACL_SIZE_INFORMATION::default();
    if !valid
        || present == 0
        || acl.is_null()
        || unsafe {
            GetAclInformation(
                acl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
    {
        unsafe { LocalFree(descriptor.cast()) };
        return "missing or invalid DACL".to_owned();
    }
    unsafe { LocalFree(descriptor.cast()) };
    format!("count={}", info.AceCount)
}

fn set_socket_owner(handle: HANDLE, sid: &[u8]) {
    // SAFETY: the handle is opened with WRITE_OWNER and `sid` is a valid user SID.
    let status = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            sid.as_ptr().cast_mut().cast(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    assert_eq!(status, 0, "set AF_UNIX owner SID");
}

fn set_directory_private_acl(path: &Path, sid: &[u8]) {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: path is NUL-terminated and the handle opens the directory itself.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    assert!(!handle.is_null() && handle != INVALID_HANDLE_VALUE);
    super::super::set_private_dacl(handle, sid).expect("set private session directory ACL");
    unsafe { CloseHandle(handle) };
}

fn sid_string(sid: &[u8]) -> String {
    sid_string_from_ptr(sid.as_ptr().cast_mut().cast())
}

fn sid_string_from_ptr(sid: *mut core::ffi::c_void) -> String {
    let mut wide = null_mut();
    assert_ne!(
        unsafe { ConvertSidToStringSidW(sid, &mut wide) },
        0,
        "format SID"
    );
    let mut length = 0;
    unsafe {
        while *wide.add(length) != 0 {
            length += 1;
        }
        let result = std::ffi::OsString::from_wide(std::slice::from_raw_parts(wide, length))
            .to_string_lossy()
            .into_owned();
        LocalFree(wide.cast());
        result
    }
}

fn session_path(root: &Path) -> PathBuf {
    let session = root
        .join("sessions")
        .join("123e4567-e89b-12d3-a456-426614174000");
    std::fs::create_dir_all(&session).expect("create session directory");
    session
}

pub(super) struct ShortStorageRoot {
    path: PathBuf,
}

impl ShortStorageRoot {
    pub(super) fn new() -> Self {
        static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
        let local_app_data = super::super::path::local_app_data().expect("Local AppData");
        const ROOT_NAMES: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        for _ in 0..ROOT_NAMES.len() {
            let index = NEXT_ROOT.fetch_add(1, Ordering::Relaxed) % ROOT_NAMES.len();
            let directory = local_app_data.join(char::from(ROOT_NAMES[index]).to_string());
            match std::fs::create_dir(&directory) {
                Ok(()) => return Self { path: directory },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create short Local AppData storage root: {error}"),
            }
        }
        panic!("unable to allocate a unique short Local AppData storage root");
    }

    pub(super) fn path(&self) -> &Path {
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
        let mut address = SOCKADDR_UN {
            sun_family: AF_UNIX,
            ..Default::default()
        };
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
            FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
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
