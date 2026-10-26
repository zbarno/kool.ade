use std::path::Path;

pub(super) fn isolated_rootless_socket(path: &Path) -> bool {
    if rootless_action_socket(path) {
        return true;
    }
    daemon_restart_socket(path)
}

pub(super) fn daemon_restart_endpoint_from_environment() -> Option<super::DockerEndpoint> {
    if std::env::var("DOCKER_CONTEXT")
        .ok()
        .is_some_and(|context| !context.trim().is_empty())
    {
        return None;
    }
    let host = std::env::var("DOCKER_HOST").ok()?;
    let path = Path::new(host.strip_prefix("unix://")?);
    daemon_restart_socket(path).then(|| super::DockerEndpoint::Unix {
        path: path.to_path_buf(),
        desktop: false,
    })
}

fn daemon_restart_socket(path: &Path) -> bool {
    if std::env::var("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION")
        .ok()
        .as_deref()
        != Some("1")
    {
        return false;
    }
    let temp_dir = std::env::temp_dir();
    if path.parent() != Some(temp_dir.as_path()) {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("koolade-docker-")?.strip_suffix(".sock"))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(unix)]
fn rootless_action_socket(path: &Path) -> bool {
    let Some(expected_socket) =
        std::env::var_os("KOOLADE_TEST_ROOTLESS_ACTION_SOCKET").map(std::path::PathBuf::from)
    else {
        return false;
    };
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return false;
    };
    rootless_action_socket_for(path, &expected_socket, &home)
}

#[cfg(unix)]
fn rootless_action_socket_for(path: &Path, expected_socket: &Path, home: &Path) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    if path.as_os_str() != expected_socket.as_os_str() {
        return false;
    }
    if std::fs::canonicalize(home).ok().as_deref() != Some(home) {
        return false;
    }
    let action_dir = home.join("setup-docker-action");
    let Some(run_dir) = path.parent() else {
        return false;
    };
    if run_dir.parent() != Some(action_dir.as_path())
        || run_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_none_or(|name| !name.starts_with("run-"))
        || path.file_name() != Some(std::ffi::OsStr::new("docker.sock"))
    {
        return false;
    }

    let uid = unsafe { libc::geteuid() };
    let owned_directory = |path: &Path| {
        std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_dir() && metadata.uid() == uid)
    };
    owned_directory(&action_dir)
        && owned_directory(run_dir)
        && std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid)
}

#[cfg(not(unix))]
fn rootless_action_socket(_path: &Path) -> bool {
    false
}

#[cfg(test)]
#[path = "test_support/tests.rs"]
mod tests;
