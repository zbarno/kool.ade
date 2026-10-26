#[cfg(unix)]
use super::rootless_action_socket_for;

#[cfg(unix)]
#[test]
fn rootless_action_socket_requires_explicit_owned_action_socket() {
    use std::{os::unix::net::UnixListener, path::Path};

    let temp = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(temp.path()).unwrap();
    let action_dir = home.join("setup-docker-action");
    let run_dir = action_dir.join("run-test");
    std::fs::create_dir_all(&run_dir).unwrap();
    let socket_path = run_dir.join("docker.sock");
    let _listener = UnixListener::bind(&socket_path).unwrap();
    assert!(rootless_action_socket_for(
        &socket_path,
        &socket_path,
        &home
    ));
    assert!(!rootless_action_socket_for(
        &socket_path,
        Path::new("/tmp/docker.sock"),
        &home
    ));
    assert!(!rootless_action_socket_for(
        Path::new("/tmp/docker.sock"),
        Path::new("/tmp/docker.sock"),
        &home
    ));
}
