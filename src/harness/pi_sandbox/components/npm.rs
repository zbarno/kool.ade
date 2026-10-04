use std::{
    env,
    path::{Component, PathBuf},
};

use super::path_safety::{
    SENSITIVE_HOST_PATH_COMPONENTS, canonical_path_with_missing_tail, ensure_narrow_host_directory,
};

/// Locate only npm's content-addressed package cache. Never mount `.npm` itself,
/// because it can contain user configuration and registry credentials.
pub(in crate::harness::pi_sandbox) fn host_npm_cache() -> anyhow::Result<Option<PathBuf>> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let configured = env::var_os("KOOLADE_NPM_CACHE").map(PathBuf::from);
    prepare_npm_cache(configured, home)
}

pub(super) fn prepare_npm_cache(
    configured: Option<PathBuf>,
    home: Option<PathBuf>,
) -> anyhow::Result<Option<PathBuf>> {
    let required = configured.is_some();
    let source = match configured {
        Some(path) if path.is_absolute() => path,
        Some(path) => env::current_dir()?.join(path),
        None => {
            let Some(home) = home.as_ref() else {
                return Ok(None);
            };
            home.join(".npm/_cacache")
        }
    };
    if !source.exists() {
        anyhow::ensure!(
            !required,
            "KOOLADE_NPM_CACHE does not identify an existing npm content cache"
        );
        return Ok(None);
    }

    let path = source.canonicalize()?;
    anyhow::ensure!(path.is_dir(), "npm cache must be a directory");
    ensure_narrow_host_directory(&path, "KOOLADE_NPM_CACHE")?;
    if let Some(home) = home.as_deref() {
        let home = canonical_path_with_missing_tail(home)?;
        anyhow::ensure!(
            path != home && !home.starts_with(&path),
            "npm cache cannot expose the host home directory or one of its parents"
        );
    }
    let leaf = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    anyhow::ensure!(
        matches!(leaf.as_str(), "_cacache" | "npm-cache" | "npm_cache"),
        "KOOLADE_NPM_CACHE must identify npm's content cache, not a general directory"
    );
    anyhow::ensure!(
        !path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .any(|name| {
                SENSITIVE_HOST_PATH_COMPONENTS
                    .iter()
                    .any(|blocked| name.eq_ignore_ascii_case(blocked))
            }),
        "npm cache cannot be inside a sensitive host directory"
    );
    anyhow::ensure!(
        ![".npmrc", "npmrc"]
            .iter()
            .any(|name| path.join(name).is_file()),
        "npm cache cannot contain npm configuration"
    );
    let temporary_root = env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| env::temp_dir());
    if path.starts_with(&temporary_root)
        || ["/tmp", "/var/tmp", "/dev/shm"]
            .iter()
            .any(|root| path.starts_with(root))
    {
        let home = home
            .as_deref()
            .map(canonical_path_with_missing_tail)
            .transpose()?;
        anyhow::ensure!(
            home.is_some_and(|home| path.starts_with(home)),
            "npm cache cannot be in a temporary directory outside the host home"
        );
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "koolade-npm-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn mounts_only_a_narrow_existing_content_cache() {
        let home = root("home");
        let cache = home.join(".npm/_cacache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(home.join(".npm/.npmrc"), "registry=https://private.invalid").unwrap();
        let found = prepare_npm_cache(None, Some(home.clone())).unwrap();
        assert_eq!(found, Some(cache.canonicalize().unwrap()));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn missing_default_cache_is_optional_but_missing_override_is_an_error() {
        let home = root("missing");
        assert_eq!(prepare_npm_cache(None, Some(home.clone())).unwrap(), None);
        assert!(prepare_npm_cache(Some(home.join("npm-cache")), Some(home)).is_err());
    }

    #[test]
    fn rejects_broad_or_sensitive_cache_directories() {
        let home = root("reject");
        let broad = home.join(".npm");
        fs::create_dir_all(&broad).unwrap();
        assert!(prepare_npm_cache(Some(broad), Some(home.clone())).is_err());
        for component in [".aws", ".pki", "keyrings", ".secret", ".secrets"] {
            let sensitive = home.join(component).join("npm-cache");
            fs::create_dir_all(&sensitive).unwrap();
            assert!(
                prepare_npm_cache(Some(sensitive), Some(home.clone())).is_err(),
                "sensitive directory {component} must not be mounted"
            );
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rejects_temporary_cache_outside_the_user_home() {
        let home = root("home");
        let outside = root("outside-home").join("npm-cache");
        fs::create_dir_all(&outside).unwrap();
        assert!(prepare_npm_cache(Some(outside.clone()), Some(home)).is_err());
        fs::remove_dir_all(outside.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_cache_symlinks_into_sensitive_directories() {
        use std::os::unix::fs::symlink;
        let home = root("symlink");
        let sensitive = home.join(".aws/npm-cache");
        fs::create_dir_all(&sensitive).unwrap();
        fs::create_dir_all(home.join(".npm")).unwrap();
        symlink(&sensitive, home.join(".npm/_cacache")).unwrap();
        assert!(prepare_npm_cache(None, Some(home.clone())).is_err());
        fs::remove_dir_all(home).unwrap();
    }
}
