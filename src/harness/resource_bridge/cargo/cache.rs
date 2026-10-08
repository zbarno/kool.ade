//! Application-owned Cargo cache with independent lockfile checksum verification.
use super::lockfile::LockedPackage;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(state_root)?;
    let state_root = state_root.canonicalize()?;
    let base = state_root.join("package-caches");
    super::ensure_private_dir(&base)?;
    let cache = base.join("cargo");
    super::ensure_private_dir(&cache)?;
    anyhow::ensure!(
        cache.canonicalize()?.starts_with(&state_root),
        "Cargo package cache escapes the Kool.ad/e state directory"
    );
    for child in [
        "registry",
        "registry/index",
        "registry/cache",
        "registry/src",
    ] {
        let path = cache.join(child);
        if !path.exists() {
            fs::create_dir(&path)?;
        }
        super::ensure_private_dir(&path)?;
        anyhow::ensure!(
            path.canonicalize()?.starts_with(&cache),
            "Cargo cache subdirectory escapes the application-owned cache"
        );
    }
    Ok(cache.canonicalize()?)
}

pub(super) fn verify_locked_packages(
    cargo_home: &Path,
    packages: &[LockedPackage],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        verified_package_count(cargo_home, packages)? == packages.len(),
        "Cargo cache is missing one or more lockfile packages"
    );
    Ok(())
}

pub(super) fn verified_package_count(
    cargo_home: &Path,
    packages: &[LockedPackage],
) -> anyhow::Result<usize> {
    let cache_root = cargo_home.join("registry/cache");
    let root = cache_root.canonicalize()?;
    anyhow::ensure!(
        root.starts_with(cargo_home.canonicalize()?),
        "Cargo registry cache escaped its application-owned directory"
    );
    let mut count = 0;
    for package in packages {
        let filename = format!("{}-{}.crate", package.name, package.version);
        let mut verified = false;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let metadata = entry.file_type()?;
            if !metadata.is_dir() || metadata.is_symlink() {
                continue;
            }
            let Some(registry_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !registry_name.starts_with("index.crates.io-") {
                continue;
            }
            let archive = entry.path().join(&filename);
            let Ok(metadata) = fs::symlink_metadata(&archive) else {
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let canonical = archive.canonicalize()?;
            anyhow::ensure!(
                canonical.starts_with(&root) && canonical.is_file(),
                "Cargo archive path escaped the application-owned cache"
            );
            if verify_sha256(&canonical, &package.checksum)? {
                verified = true;
                break;
            }
            anyhow::bail!(
                "Cargo archive {} did not match its Cargo.lock SHA-256 checksum",
                package.name
            );
        }
        count += usize::from(verified);
    }
    Ok(count)
}

fn verify_sha256(path: &Path, expected: &str) -> anyhow::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(to_hex(&digest.finalize()).eq_ignore_ascii_case(expected))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{verify_locked_packages, verify_sha256};
    use crate::harness::resource_bridge::cargo::lockfile::LockedPackage;
    use sha2::{Digest, Sha256};
    use std::fs;

    #[test]
    fn verifies_cargo_archive_checksum_before_cache_is_exposed() {
        let root =
            std::env::temp_dir().join(format!("koolade-cargo-cache-{}", uuid::Uuid::new_v4()));
        let cache = root.join("registry/cache/index.crates.io-test");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join("serde-1.0.0.crate");
        fs::write(&archive, b"locked crate archive").unwrap();
        let checksum = super::to_hex(&Sha256::digest(b"locked crate archive"));
        let package = LockedPackage {
            name: "serde".into(),
            version: "1.0.0".into(),
            checksum: checksum.clone(),
        };
        assert!(verify_sha256(&archive, &checksum).unwrap());
        verify_locked_packages(&root, std::slice::from_ref(&package)).unwrap();
        fs::write(&archive, b"tampered crate archive").unwrap();
        assert!(!verify_sha256(&archive, &checksum).unwrap());
        assert!(verify_locked_packages(&root, &[package]).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
