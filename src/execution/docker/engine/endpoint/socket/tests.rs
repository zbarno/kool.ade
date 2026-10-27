use super::*;
use std::os::unix::{
    fs::{PermissionsExt, symlink},
    net::UnixListener,
};

fn desktop_fixture() -> (tempfile::TempDir, std::path::PathBuf, UnixListener) {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap();
    let root = tempfile::tempdir_in(home).unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let docker_dir = root.path().join(".docker");
    std::fs::create_dir(&docker_dir).unwrap();
    std::fs::set_permissions(&docker_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let run_dir = docker_dir.join("run");
    std::fs::create_dir(&run_dir).unwrap();
    std::fs::set_permissions(&run_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = run_dir.join("docker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    (root, socket, listener)
}

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
        Err(SocketFilesystemError::Untrusted)
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
        Err(SocketFilesystemError::Untrusted)
    );
}

#[test]
fn rejects_a_socket_not_owned_by_the_expected_user() {
    let (runtime_dir, socket, _listener) = rootless_fixture();
    let other_uid = unsafe { libc::geteuid() }.saturating_add(1);

    assert_eq!(
        validate_rootless_socket_at(runtime_dir.path(), &socket, other_uid),
        Err(SocketFilesystemError::Untrusted)
    );
}

#[test]
fn accepts_desktop_socket_with_private_current_user_parent_chain() {
    let (_root, socket, _listener) = desktop_fixture();

    validate_unix_socket(&socket, true).unwrap();
}

#[test]
fn rejects_desktop_socket_with_group_or_other_permissions() {
    for mode in [0o620, 0o604] {
        let (_root, socket, _listener) = desktop_fixture();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(mode)).unwrap();

        let error = validate_unix_socket(&socket, true).unwrap_err();

        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    }
}

#[test]
fn rejects_a_symlink_in_the_desktop_socket_parent_chain() {
    let (root, socket, _listener) = desktop_fixture();
    let docker_dir = root.path().join(".docker");
    let real_docker_dir = root.path().join(".docker-real");
    std::fs::rename(&docker_dir, &real_docker_dir).unwrap();
    symlink(&real_docker_dir, &docker_dir).unwrap();

    let error = validate_unix_socket(&socket, true).unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
}

#[test]
fn rejects_group_or_other_writable_desktop_parent_directories() {
    for mode in [0o770, 0o707] {
        let (_root, socket, _listener) = desktop_fixture();
        let docker_dir = socket.parent().unwrap().parent().unwrap();
        std::fs::set_permissions(docker_dir, std::fs::Permissions::from_mode(mode)).unwrap();

        let error = validate_unix_socket(&socket, true).unwrap_err();

        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    }
}

#[test]
fn checks_writable_permissions_on_ancestors_above_the_docker_directory() {
    let (root, socket, _listener) = desktop_fixture();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o770)).unwrap();

    let error = validate_unix_socket(&socket, true).unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
}

#[test]
fn rejects_desktop_parent_directories_owned_by_another_user() {
    let (root, socket, _listener) = desktop_fixture();
    let current_uid = unsafe { libc::geteuid() };
    if current_uid == 0 {
        return;
    }

    assert_eq!(
        validate_desktop_socket_parent_chain(&socket, current_uid.saturating_add(1)),
        Err(SocketFilesystemError::Untrusted)
    );
    drop(root);
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_an_acl_on_any_desktop_socket_parent_directory() {
    use std::process::Command;

    for parent_offset in [1, 2] {
        let (_root, socket, _listener) = desktop_fixture();
        let mut parent = socket.parent().unwrap();
        for _ in 1..parent_offset {
            parent = parent.parent().unwrap();
        }
        let status = Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone allow read")
            .arg(parent)
            .status()
            .unwrap();
        assert!(status.success(), "could not install test ACL");

        let error = validate_unix_socket(&socket, true).unwrap_err();

        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    }
}

#[cfg(target_os = "macos")]
#[test]
// Covers the real socket-vnode ACL lookup used by the production Desktop endpoint validator.
// A unit mock cannot prove macOS observes an ACL-only grant while POSIX mode stays private.
// Requires macOS with an ACL-capable filesystem and /bin/chmod; no Docker or network. Setup is low cost.
fn rejects_an_acl_on_the_desktop_socket_node() {
    use std::os::unix::fs::MetadataExt;
    use std::process::Command;

    let (_root, socket, _listener) = desktop_fixture();
    let status = Command::new("/bin/chmod")
        .arg("+a")
        .arg("everyone allow readsecurity")
        .arg(&socket)
        .status()
        .unwrap();
    assert!(status.success(), "could not install test ACL on socket");
    assert_eq!(
        std::fs::symlink_metadata(&socket).unwrap().mode() & 0o077,
        0
    );

    let error = validate_unix_socket(&socket, true).unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
}

#[cfg(target_os = "macos")]
#[test]
fn fails_closed_when_the_acl_api_cannot_inspect_a_directory() {
    assert_eq!(
        validate_macos_acl_fd(-1),
        Err(SocketFilesystemError::Untrusted)
    );
}
