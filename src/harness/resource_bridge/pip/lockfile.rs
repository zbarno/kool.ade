use crate::harness::DependencyPackageIdentity;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

const MAX_REQUIREMENTS_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REQUIREMENT_LINE_BYTES: usize = 4 * 1024;
const MAX_LOCKED_PACKAGES: usize = 2_000;
const MAX_HASHES_PER_PACKAGE: usize = 16;
const PYPI_SOURCE: &str = "https://pypi.org";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockedPackage {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) hashes: BTreeSet<String>,
}

pub(super) fn collect(root: &Path) -> anyhow::Result<Vec<LockedPackage>> {
    let root = root.canonicalize()?;
    anyhow::ensure!(root.is_dir(), "Pip dependency root is not a directory");
    let path = root.join("requirements.txt");
    let file = super::super::secure_file::open_read_nofollow(&path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "requirements.txt must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_REQUIREMENTS_BYTES,
        "requirements.txt exceeds the 2 MiB scan limit"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_REQUIREMENTS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_REQUIREMENTS_BYTES,
        "requirements.txt grew beyond the 2 MiB scan limit"
    );
    let contents = String::from_utf8(bytes)?;
    collect_contents(&contents)
}

pub(super) fn package_identities(root: &Path) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    Ok(collect(root)?.into_iter().map(package_identity).collect())
}

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    Ok(collect_contents(contents)?
        .into_iter()
        .map(package_identity)
        .collect())
}

pub(super) fn identity(packages: &[LockedPackage]) -> anyhow::Result<String> {
    let identities = packages
        .iter()
        .cloned()
        .map(package_identity)
        .collect::<Vec<_>>();
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&(
            identities,
            Option::<Vec<(String, serde_json::Value)>>::None
        ))?)
    ))
}

#[cfg(unix)]
pub(super) fn write_canonical_snapshot(
    path: &Path,
    packages: &[LockedPackage],
) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    for package in packages {
        write!(file, "{}=={}", package.name, package.version)?;
        for hash in &package.hashes {
            write!(file, " --hash=sha256:{hash}")?;
        }
        writeln!(file)?;
    }
    file.sync_all()?;
    Ok(())
}

fn package_identity(package: LockedPackage) -> DependencyPackageIdentity {
    DependencyPackageIdentity {
        package: package.name,
        version: package.version,
        source: PYPI_SOURCE.into(),
        integrity: format!(
            "sha256:{}",
            package.hashes.into_iter().collect::<Vec<_>>().join(",")
        ),
    }
}

fn collect_contents(contents: &str) -> anyhow::Result<Vec<LockedPackage>> {
    anyhow::ensure!(
        contents.len() as u64 <= MAX_REQUIREMENTS_BYTES,
        "requirements.txt exceeds the 2 MiB scan limit"
    );
    let mut packages = BTreeMap::<String, LockedPackage>::new();
    for (line_number, line) in contents.lines().enumerate() {
        anyhow::ensure!(
            line.len() <= MAX_REQUIREMENT_LINE_BYTES,
            "requirements.txt line {} exceeds the scan limit",
            line_number + 1
        );
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        anyhow::ensure!(
            !line
                .chars()
                .any(|character| matches!(character, '\\' | ';' | '#'))
                && line.is_ascii()
                && !line.starts_with('-'),
            "requirements.txt contains an unsupported option, marker, include, or escape"
        );
        let mut tokens = line.split_ascii_whitespace();
        let requirement = tokens
            .next()
            .ok_or_else(|| anyhow::anyhow!("requirements.txt contains an empty entry"))?;
        let (raw_name, version) = requirement
            .split_once("==")
            .ok_or_else(|| anyhow::anyhow!("Pip restore requires exact name==version pins"))?;
        anyhow::ensure!(
            !version.contains('=') && valid_version(version) && normalize_name(raw_name).is_ok(),
            "requirements.txt contains a non-exact or invalid package identity"
        );
        let name = normalize_name(raw_name)?;
        let mut hashes = BTreeSet::new();
        let mut hash_count = 0;
        for token in tokens {
            let hash = token
                .strip_prefix("--hash=sha256:")
                .ok_or_else(|| anyhow::anyhow!("Pip restore accepts SHA-256 hashes only"))?;
            anyhow::ensure!(
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "requirements.txt contains an invalid SHA-256 wheel hash"
            );
            hash_count += 1;
            anyhow::ensure!(
                hash_count <= MAX_HASHES_PER_PACKAGE,
                "requirements.txt contains too many hashes for one package"
            );
            hashes.insert(hash.to_ascii_lowercase());
        }
        anyhow::ensure!(
            !hashes.is_empty(),
            "every Pip requirement must include at least one SHA-256 hash"
        );
        anyhow::ensure!(
            packages
                .insert(
                    name.clone(),
                    LockedPackage {
                        name,
                        version: version.to_ascii_lowercase(),
                        hashes,
                    },
                )
                .is_none(),
            "requirements.txt contains a duplicate normalized package name"
        );
        anyhow::ensure!(
            packages.len() <= MAX_LOCKED_PACKAGES,
            "requirements.txt contains too many packages"
        );
    }
    Ok(packages.into_values().collect())
}

pub(super) fn normalize_name(value: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 214,
        "Pip package name is empty or too long"
    );
    anyhow::ensure!(
        value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && value
                .bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric()),
        "Pip package name must begin and end with a letter or digit"
    );
    let mut normalized = String::with_capacity(value.len());
    let mut separator = false;
    for byte in value.bytes() {
        if matches!(byte, b'-' | b'_' | b'.') {
            separator = !normalized.is_empty();
            continue;
        }
        anyhow::ensure!(
            byte.is_ascii_alphanumeric(),
            "Pip package name contains an invalid character"
        );
        if separator {
            normalized.push('-');
            separator = false;
        }
        normalized.push((byte as char).to_ascii_lowercase());
    }
    anyhow::ensure!(
        !normalized.is_empty() && normalized.len() <= 214,
        "Pip package name is invalid"
    );
    Ok(normalized)
}

pub(super) fn normalize_version(value: &str) -> anyhow::Result<String> {
    anyhow::ensure!(valid_version(value), "Pip wheel version is invalid");
    Ok(value.to_ascii_lowercase())
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_'))
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
#[path = "lockfile/tests.rs"]
mod tests;
