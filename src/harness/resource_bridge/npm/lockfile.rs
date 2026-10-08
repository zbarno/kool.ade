//! Bounded npm lockfile scanning and integrity helpers.
use serde_json::Value;
mod identity;
mod integrity;
pub(super) use identity::{packages as package_identities, packages_from_bytes};
#[cfg(test)]
pub(super) use integrity::cache_covers_lockfile;
#[cfg(test)]
pub(super) use integrity::decode_base64;
pub(super) use integrity::{npm_cache_digest_path, verify_sha512_file};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::Path,
};

const MAX_LOCKFILES: usize = 128;
const MAX_LOCKFILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LOCK_TREE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LOCK_DEPTH: usize = 20;
pub(super) const MAX_LOCKED_PACKAGES: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockedPackage {
    pub(super) url: String,
    pub(super) integrity: String,
}

pub(super) fn is_remote_http_url(value: &str) -> bool {
    value.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

pub(super) fn collect_lockfiles(root: &Path) -> anyhow::Result<Vec<LockedPackage>> {
    collect_lockfiles_with_requirement(root, true)
}

pub(super) fn collect_lockfiles_if_present(root: &Path) -> anyhow::Result<Vec<LockedPackage>> {
    collect_lockfiles_with_requirement(root, false)
}

pub(super) fn has_root_lockfile(root: &Path) -> anyhow::Result<bool> {
    let root = root.canonicalize()?;
    for name in ["package-lock.json", "npm-shrinkwrap.json"] {
        let path = root.join(name);
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "Root npm lockfile must be a regular file"
                );
                return Ok(true);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

fn collect_lockfiles_with_requirement(
    root: &Path,
    require_lockfile: bool,
) -> anyhow::Result<Vec<LockedPackage>> {
    let root = root.canonicalize()?;
    let mut pending = VecDeque::from([(root.clone(), 0_usize)]);
    let mut lockfiles = Vec::new();
    let mut total_bytes = 0_u64;
    while let Some((directory, depth)) = pending.pop_front() {
        anyhow::ensure!(
            depth <= MAX_LOCK_DEPTH,
            "project tree exceeds the npm lockfile scan depth"
        );
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                if !matches!(
                    name.to_str(),
                    Some(
                        ".git"
                            | "node_modules"
                            | ".koolade-packet"
                            | "target"
                            | "dist"
                            | "build"
                            | "coverage"
                            | "vendor"
                            | "obj"
                            | "bin"
                    )
                ) {
                    pending.push_back((path, depth + 1));
                }
                continue;
            }
            if !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("package-lock.json" | "npm-shrinkwrap.json")
            ) {
                continue;
            }
            anyhow::ensure!(
                lockfiles.len() < MAX_LOCKFILES,
                "project contains too many npm lockfiles"
            );
            let metadata = fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= MAX_LOCKFILE_BYTES,
                "npm lockfile is larger than 32 MiB"
            );
            total_bytes = total_bytes.saturating_add(metadata.len());
            anyhow::ensure!(
                total_bytes <= MAX_LOCK_TREE_BYTES,
                "npm lockfiles exceed the 128 MiB scan budget"
            );
            lockfiles.push(path);
        }
    }
    if lockfiles.is_empty() {
        anyhow::ensure!(
            !require_lockfile,
            "No package-lock.json or npm-shrinkwrap.json was found. Create a lockfile before installing npm dependencies."
        );
        return Ok(Vec::new());
    }
    let mut packages = BTreeMap::<String, LockedPackage>::new();
    for lockfile in lockfiles {
        let value: Value = serde_json::from_slice(&fs::read(lockfile)?)?;
        collect_resolved(&value, &mut packages)?;
        anyhow::ensure!(
            packages.len() <= MAX_LOCKED_PACKAGES,
            "npm lockfiles contain more than {MAX_LOCKED_PACKAGES} remote packages"
        );
    }
    Ok(packages.into_values().collect())
}

pub(super) fn contains_verified_identity(root: &Path, name: &str, version: &str) -> bool {
    if !exact_version(version) {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    ["package-lock.json", "npm-shrinkwrap.json"]
        .iter()
        .any(|namefile| {
            let path = root.join(namefile);
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                return false;
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_LOCKFILE_BYTES
            {
                return false;
            }
            let Ok(value) = fs::read(&path).and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(std::io::Error::other)
            }) else {
                return false;
            };
            has_locked_identity(&value, name, version)
        })
}

fn has_locked_identity(value: &Value, name: &str, version: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            let named = key == name
                || key == &format!("node_modules/{name}")
                || key.ends_with(&format!("/node_modules/{name}"));
            if named
                && child.get("version").and_then(Value::as_str) == Some(version)
                && child
                    .get("integrity")
                    .and_then(Value::as_str)
                    .is_some_and(|integrity| integrity.starts_with("sha512-"))
                && child
                    .get("resolved")
                    .and_then(Value::as_str)
                    .is_some_and(|url| {
                        matches!(
                            super::super::policy::classify(url),
                            Ok(super::super::policy::Decision::Allow(_))
                        )
                    })
            {
                return true;
            }
            has_locked_identity(child, name, version)
        }),
        Value::Array(values) => values
            .iter()
            .any(|child| has_locked_identity(child, name, version)),
        _ => false,
    }
}

fn exact_version(version: &str) -> bool {
    version.len() <= 128
        && version.contains('.')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
}

fn collect_resolved(
    value: &Value,
    packages: &mut BTreeMap<String, LockedPackage>,
) -> anyhow::Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(resolved) = object.get("resolved").and_then(Value::as_str) {
                let is_link = object.get("link").and_then(Value::as_bool) == Some(true);
                if is_link {
                    anyhow::ensure!(
                        !is_remote_http_url(resolved),
                        "npm lockfile link entries cannot reference remote URLs"
                    );
                }
                if is_link || resolved.starts_with("file:") || resolved.starts_with("link:") {
                    // Workspace and local-file packages need no registry fetch.
                } else {
                    let integrity = object
                        .get("integrity")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("Locked package has no integrity digest"))?;
                    if let Some(previous) = packages.get(resolved) {
                        anyhow::ensure!(
                            previous.integrity == integrity,
                            "Lockfiles disagree on the integrity digest for a package"
                        );
                    } else {
                        packages.insert(
                            resolved.to_owned(),
                            LockedPackage {
                                url: resolved.to_owned(),
                                integrity: integrity.to_owned(),
                            },
                        );
                    }
                }
            }
            for child in object.values() {
                collect_resolved(child, packages)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_resolved(child, packages)?;
            }
        }
        _ => {}
    }
    Ok(())
}
