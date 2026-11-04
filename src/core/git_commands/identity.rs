use std::path::Path;

pub(crate) fn remote_identity_for(repo: &Path, remote: &str) -> String {
    if let Ok(url) = url::Url::parse(remote) {
        if url.scheme() == "file"
            && let Ok(path) = url.to_file_path()
        {
            return local_identity(&path);
        }
        if let Some(host) = url.host_str() {
            let port = url
                .port()
                .filter(|port| Some(*port) != url.port_or_known_default())
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            return format!(
                "git://{}{}{}",
                host.to_ascii_lowercase(),
                port,
                normalize_repository_path(url.path())
            );
        }
    }
    if let Some((user, host_path)) = remote.split_once('@')
        && valid_scp_user(user)
        && let Some((host, path)) = host_path.split_once(':')
        && !host.is_empty()
        && !host.contains('/')
    {
        let path = normalize_repository_path(path);
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        return format!("git://{}{}", host.to_ascii_lowercase(), path);
    }
    local_identity(&if Path::new(remote).is_absolute() {
        Path::new(remote).to_path_buf()
    } else {
        repo.join(remote)
    })
}

fn local_identity(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file:{}", path.to_string_lossy())
}

fn normalize_repository_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    path.strip_suffix(".git").unwrap_or(path).to_owned()
}

fn valid_scp_user(user: &str) -> bool {
    !user.is_empty()
        && user
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}
