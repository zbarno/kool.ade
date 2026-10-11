use super::output;
use std::path::Path;

pub(in crate::core::planning_sync) fn remote_is_configured(repo: &Path) -> anyhow::Result<bool> {
    Ok(remote_origin(repo)?.is_some())
}

pub(in crate::core::planning_sync) fn remote_identity(
    repo: &Path,
) -> anyhow::Result<Option<String>> {
    let Some(remote) = remote_origin(repo)? else {
        return Ok(None);
    };
    Ok(Some(remote_identity_for(repo, &remote)))
}

pub(in crate::core::planning_sync) fn remote_identity_for(repo: &Path, remote: &str) -> String {
    normalize_remote_identity(repo, remote)
}

pub(in crate::core::planning_sync) fn is_canonical_branch(
    repo: &Path,
    canonical_ref: &str,
) -> anyhow::Result<bool> {
    let Some(expected) = canonical_ref.strip_prefix("refs/heads/") else {
        return Ok(false);
    };
    let current = output(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    Ok(current.status.success() && String::from_utf8_lossy(&current.stdout).trim() == expected)
}

fn remote_origin(repo: &Path) -> anyhow::Result<Option<String>> {
    let result = output(repo, &["remote", "get-url", "origin"])?;
    Ok(result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .filter(|remote| !remote.is_empty()))
}

pub(in crate::core::planning_sync) fn set_origin(repo: &Path, remote: &str) -> anyhow::Result<()> {
    let arguments = if remote_origin(repo)?.is_some() {
        vec!["remote", "set-url", "origin", remote]
    } else {
        vec!["remote", "add", "origin", remote]
    };
    let result = output(repo, &arguments)?;
    anyhow::ensure!(
        result.status.success(),
        "planning remote configuration failed"
    );
    Ok(())
}

fn normalize_remote_identity(repo: &Path, remote: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::normalize_remote_identity;
    use std::path::Path;

    #[test]
    fn equivalent_network_remote_forms_share_one_identity() {
        let repo = Path::new(".");
        let https = normalize_remote_identity(repo, "https://git.example.test/team/planning.git");
        let ssh = normalize_remote_identity(repo, "ssh://git@git.example.test/team/planning/");
        let scp = normalize_remote_identity(repo, "git@git.example.test:team/planning.git");
        assert_eq!(https, ssh);
        assert_eq!(https, scp);
    }

    #[test]
    fn equivalent_local_remote_paths_share_one_identity() {
        let root = std::env::temp_dir().join(format!("planning-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let with_slash = format!("{}/", root.display());
        let first = normalize_remote_identity(&root, root.to_str().unwrap());
        let second = normalize_remote_identity(&root, &with_slash);
        assert_eq!(first, second);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_paths_containing_scp_like_text_remain_local_identities() {
        let remote = "/tmp/git@host.example.test:team/planning.git";
        let identity = normalize_remote_identity(Path::new("."), remote);
        assert_eq!(identity, format!("file:{remote}"));
        assert_ne!(
            identity,
            normalize_remote_identity(Path::new("."), "git@host.example.test:team/planning.git")
        );
    }
}
