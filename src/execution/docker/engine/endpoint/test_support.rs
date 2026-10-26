use std::path::Path;

pub(super) fn isolated_rootless_socket(path: &Path) -> bool {
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
