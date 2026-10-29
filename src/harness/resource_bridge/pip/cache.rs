use super::lockfile::LockedPackage;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

mod identity;
use identity::{identity_directory, lockfile_digest, sha256_file};

const MAX_WHEEL_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(state_root)?;
    let state_root = state_root.canonicalize()?;
    let package_caches = state_root.join("package-caches");
    ensure_private_directory(&package_caches)?;
    let cache = package_caches.join("pip");
    ensure_private_directory(&cache)?;
    anyhow::ensure!(
        cache.canonicalize()?.starts_with(&state_root),
        "Pip wheelhouse escapes the Kool.ad/e state directory"
    );
    Ok(cache.canonicalize()?)
}

pub(super) fn snapshot_root(cache_root: &Path, lockfile_identity: &str) -> anyhow::Result<PathBuf> {
    let identity_dir = identity_directory(cache_root, lockfile_identity)?;
    ensure_real_directory(&identity_dir)?;
    let wheelhouse = identity_dir.join("wheels");
    ensure_real_directory(&wheelhouse)?;
    let canonical = identity_dir.canonicalize()?;
    anyhow::ensure!(
        canonical.starts_with(cache_root.canonicalize()?),
        "Pip cache snapshot escaped its application-owned root"
    );
    Ok(canonical)
}

pub(super) fn verified_wheel_count(
    cache_root: &Path,
    lockfile_identity: &str,
    packages: &[LockedPackage],
) -> anyhow::Result<usize> {
    let identity_dir = identity_directory(cache_root, lockfile_identity)?;
    match fs::symlink_metadata(&identity_dir) {
        Ok(_) => ensure_real_directory(&identity_dir)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    }
    anyhow::ensure!(
        identity_dir
            .canonicalize()?
            .starts_with(cache_root.canonicalize()?),
        "Pip identity cache escaped its application-owned root"
    );
    let wheelhouse = identity_dir.join("wheels");
    if !wheelhouse.exists() {
        return Ok(0);
    }
    verify_wheelhouse(&wheelhouse, packages)
}

pub(super) fn publish_wheelhouse(
    cache_root: &Path,
    lockfile_identity: &str,
    staging_wheelhouse: &Path,
    packages: &[LockedPackage],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        verify_wheelhouse(staging_wheelhouse, packages)? == packages.len(),
        "Pip did not prepare exactly the hash-locked wheel set"
    );
    let cache_root = cache_root.canonicalize()?;
    let identity = lockfile_digest(lockfile_identity)?;
    let identities = cache_root.join("sha256");
    ensure_private_directory(&identities)?;
    let destination = identities.join(&identity);
    let temporary = identities.join(format!(".staging-{identity}-{}", uuid::Uuid::new_v4()));
    create_private_dir(&temporary)?;
    let result = (|| {
        let wheels = temporary.join("wheels");
        create_private_dir(&wheels)?;
        copy_verified_wheels(staging_wheelhouse, &wheels)?;
        anyhow::ensure!(
            verify_wheelhouse(&wheels, packages)? == packages.len(),
            "Published Pip wheelhouse failed independent verification"
        );
        replace_directory(&temporary, &destination, &identities, &identity, packages)?;
        Ok(())
    })();
    if temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn replace_directory(
    temporary: &Path,
    destination: &Path,
    identities: &Path,
    identity: &str,
    packages: &[LockedPackage],
) -> anyhow::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Pip identity cache is not a real directory"
            );
            if verify_wheelhouse(&destination.join("wheels"), packages)
                .is_ok_and(|count| count == packages.len())
            {
                return Ok(());
            }
            let backup = identities.join(format!(".invalid-{identity}-{}", uuid::Uuid::new_v4()));
            fs::rename(destination, &backup)?;
            if let Err(error) = fs::rename(temporary, destination) {
                let _ = fs::rename(&backup, destination);
                return Err(error.into());
            }
            fs::remove_dir_all(backup)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(temporary, destination)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn copy_verified_wheels(source: &Path, destination: &Path) -> anyhow::Result<()> {
    ensure_real_directory(source)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        anyhow::ensure!(
            kind.is_file() && !kind.is_symlink(),
            "Pip wheelhouse contains a symlink or special file"
        );
        let target = destination.join(entry.file_name());
        let mut input = fs::File::open(entry.path())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(target)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    Ok(())
}

pub(super) fn verify_wheelhouse(
    wheelhouse: &Path,
    packages: &[LockedPackage],
) -> anyhow::Result<usize> {
    if !wheelhouse.exists() {
        return Ok(0);
    }
    ensure_real_directory(wheelhouse)?;
    let expected = packages
        .iter()
        .map(|package| ((package.name.as_str(), package.version.as_str()), package))
        .collect::<BTreeMap<_, _>>();
    let mut verified = BTreeSet::new();
    let mut total_bytes = 0_u64;
    let mut file_count = 0_usize;
    for entry in fs::read_dir(wheelhouse)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        anyhow::ensure!(
            kind.is_file() && !kind.is_symlink(),
            "Pip wheelhouse contains a symlink or special file"
        );
        let metadata = entry.metadata()?;
        anyhow::ensure!(
            metadata.len() <= MAX_WHEEL_BYTES,
            "Pip wheel exceeds the 64 MiB per-file limit"
        );
        total_bytes = total_bytes.saturating_add(metadata.len());
        anyhow::ensure!(
            total_bytes <= crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES,
            "Pip wheelhouse exceeds the cache snapshot limit"
        );
        let file_name = entry.file_name();
        let file_name = file_name
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Pip wheel filename is not valid UTF-8"))?;
        let (name, version) = wheel_identity(file_name)?;
        let package = expected
            .get(&(name.as_str(), version.as_str()))
            .ok_or_else(|| anyhow::anyhow!("Pip produced a wheel outside requirements.txt"))?;
        let digest = sha256_file(&entry.path())?;
        anyhow::ensure!(
            package.hashes.contains(&digest),
            "Pip wheel SHA-256 did not match requirements.txt"
        );
        anyhow::ensure!(
            verified.insert((name, version)),
            "Pip produced more than one wheel for a locked package"
        );
        file_count += 1;
        anyhow::ensure!(file_count <= packages.len(), "Pip produced too many wheels");
    }
    Ok(file_count)
}

fn wheel_identity(file_name: &str) -> anyhow::Result<(String, String)> {
    let stem = file_name
        .strip_suffix(".whl")
        .ok_or_else(|| anyhow::anyhow!("Pip produced a source archive instead of a wheel"))?;
    let fields = stem.split('-').collect::<Vec<_>>();
    anyhow::ensure!(
        fields.len() == 5
            && !fields[0].is_empty()
            && !fields[1].is_empty()
            && matches!(fields[2], "py3" | "py2.py3")
            && fields[3] == "none"
            && fields[4] == "any",
        "Pip produced a non-portable wheel; only universal Python 3 wheels are supported"
    );
    Ok((
        super::lockfile::normalize_name(fields[0])?,
        super::lockfile::normalize_version(fields[1])?,
    ))
}

fn ensure_real_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Pip cache path is not a real directory"
    );
    Ok(())
}

fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Pip cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {}
            Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)?;
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Pip cache path contains a symlink or non-directory"
                );
            }
            Err(create_error) => return Err(create_error.into()),
        },
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
#[path = "cache/tests.rs"]
mod tests;
