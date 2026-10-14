use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn locate_git(root: &Path) -> anyhow::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let candidate = directory.join("git");
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if candidate.is_file() && !candidate.starts_with(root) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if candidate.metadata()?.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return Ok(candidate);
        }
    }
    anyhow::bail!("Cannot locate a trusted Git executable outside the repository")
}

pub(super) fn git_path(root: &Path, option: &str) -> anyhow::Result<PathBuf> {
    let output = Command::new(locate_git(root)?)
        .args(["rev-parse", "--path-format=absolute", option])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot safely locate this repository's Git metadata"
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).canonicalize()?;
    anyhow::ensure!(path.is_dir(), "Git metadata path is not a directory");
    Ok(path)
}

pub(super) fn validate_koolade_clone(root: &Path, common: &Path) -> anyhow::Result<()> {
    let projects = crate::persistence::state_root()
        .join("projects")
        .canonicalize()?;
    anyhow::ensure!(
        root.starts_with(&projects),
        "Task clone is outside Kool.ad/e's private project storage"
    );
    let relative = root.strip_prefix(&projects)?;
    let parts = relative.components().collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() == 4
            && parts[1].as_os_str() == "task-repositories"
            && valid_repository_component(parts[2].as_os_str())
            && valid_task_repository_component(parts[3].as_os_str())
            && !parts[0].as_os_str().is_empty()
            && common == root.join(".git"),
        "Task clone path does not match Kool.ad/e's allocation"
    );
    Ok(())
}

fn valid_repository_component(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 40
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn valid_task_repository_component(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let valid_key = |base: &str| {
        base.rsplit_once('-').is_some_and(|(slug, hash)| {
            !slug.is_empty()
                && hash.len() == 16
                && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    for marker in ["-integration-", "-verification-"] {
        if let Some((base, suffix)) = name.rsplit_once(marker)
            && suffix.len() == 12
            && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return valid_key(base);
        }
    }
    valid_key(name)
}
