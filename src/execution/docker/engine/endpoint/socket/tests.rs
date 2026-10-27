use super::*;
use std::os::unix::{fs::PermissionsExt, net::UnixListener};

fn rootless_fixture() -> (tempfile::TempDir, std::path::PathBuf, UnixListener) {
    let runtime_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = runtime_dir.path().join("docker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660)).unwrap();
    (runtime_dir, socket, listener)
}

#[test]
fn accepts_normal_rootless_socket_mode_inside_a_private_runtime_directory() {
    let (runtime_dir, socket, _listener) = rootless_fixture();
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return;
    }
    for mode in [0o600, 0o660] {
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(mode)).unwrap();
        validate_rootless_socket_at(runtime_dir.path(), &socket, uid).unwrap();
    }
}

#[test]
fn rejects_a_runtime_directory_that_is_not_private() {
    let (runtime_dir, socket, _listener) = rootless_fixture();
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return;
    }
    std::fs::set_permissions(runtime_dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        validate_rootless_socket_at(runtime_dir.path(), &socket, uid),
        Err(RootlessSocketError::Untrusted)
    );
}

#[test]
fn rejects_a_socket_that_is_writable_by_group_or_other_users() {
    let (runtime_dir, socket, _listener) = rootless_fixture();
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return;
    }
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();

    assert_eq!(
        validate_rootless_socket_at(runtime_dir.path(), &socket, uid),
        Err(RootlessSocketError::Untrusted)
    );
}

#[test]
fn rejects_a_socket_not_owned_by_the_expected_user() {
    let (runtime_dir, socket, _listener) = rootless_fixture();
    let other_uid = unsafe { libc::geteuid() }.saturating_add(1);

    assert_eq!(
        validate_rootless_socket_at(runtime_dir.path(), &socket, other_uid),
        Err(RootlessSocketError::Untrusted)
    );
}
