use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(state_root)?;
    let state_root = state_root.canonicalize()?;
    let package_caches = state_root.join("package-caches");
    ensure_private_directory(&package_caches)?;
    let cache = package_caches.join("go");
    ensure_private_directory(&cache)?;
    let canonical = cache.canonicalize()?;
    anyhow::ensure!(
        canonical.starts_with(&state_root),
        "Go module cache escapes the Kool.ad/e state directory"
    );
    Ok(canonical)
}

pub(super) fn snapshot_root(cache_root: &Path, lockfile_identity: &str) -> anyhow::Result<PathBuf> {
    let digest = lockfile_identity
        .strip_prefix("sha256:")
        .ok_or_else(|| anyhow::anyhow!("Go lockfile identity has an unsupported format"))?;
    anyhow::ensure!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Go lockfile identity is not a SHA-256 digest"
    );
    let identity_root = cache_root.join("sha256").join(digest.to_ascii_lowercase());
    ensure_private_directory(identity_root.parent().expect("identity parent"))?;
    ensure_private_directory(&identity_root)?;
    anyhow::ensure!(
        identity_root
            .canonicalize()?
            .starts_with(cache_root.canonicalize()?),
        "Go module snapshot escaped its application-owned cache"
    );
    Ok(identity_root)
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Go module cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {}
            Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)?;
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Go module cache path contains a symlink or non-directory"
                );
            }
            Err(create_error) => return Err(create_error.into()),
        },
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
