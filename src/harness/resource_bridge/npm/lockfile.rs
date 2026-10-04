//! Bounded npm lockfile scanning and integrity helpers.
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
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

pub(super) fn collect_lockfiles(root: &Path) -> anyhow::Result<Vec<LockedPackage>> {
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
    anyhow::ensure!(
        !lockfiles.is_empty(),
        "No package-lock.json or npm-shrinkwrap.json was found. Create a lockfile before installing npm dependencies."
    );
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

fn collect_resolved(
    value: &Value,
    packages: &mut BTreeMap<String, LockedPackage>,
) -> anyhow::Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(resolved) = object.get("resolved").and_then(Value::as_str) {
                if resolved.starts_with("file:") || resolved.starts_with("link:") {
                    // Workspace and local-file packages need no registry fetch.
                } else {
                    let integrity =
                        object
                            .get("integrity")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                anyhow::anyhow!("Locked package {resolved} has no integrity digest")
                            })?;
                    if let Some(previous) = packages.get(resolved) {
                        anyhow::ensure!(
                            previous.integrity == integrity,
                            "Lockfiles disagree on the integrity digest for {resolved}"
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

pub(super) fn npm_cache_digest_path(cache: &Path, integrity: &str) -> Option<PathBuf> {
    let sri = integrity
        .split_whitespace()
        .find_map(|token| token.strip_prefix("sha512-"))?;
    let encoded = sri.split_once('?').map_or(sri, |(digest, _)| digest);
    let digest = decode_base64(encoded)?;
    if digest.len() != 64 {
        return None;
    }
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(
        cache
            .join("_cacache/content-v2/sha512")
            .join(&hex[..2])
            .join(&hex[2..4])
            .join(&hex[4..]),
    )
}

pub(super) fn verify_sha512_file(path: &Path, integrity: &str) -> bool {
    let Some(encoded) = integrity
        .split_whitespace()
        .find_map(|token| token.strip_prefix("sha512-"))
    else {
        return false;
    };
    let encoded = encoded
        .split_once('?')
        .map_or(encoded, |(digest, _)| digest);
    let Some(expected) = decode_base64(encoded) else {
        return false;
    };
    if expected.len() != 64 {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    Sha512::digest(bytes).as_slice() == expected
}

pub(super) fn cache_covers_lockfile(worktree: &Path, cache: &Path) -> bool {
    let Ok(packages) = collect_lockfiles(worktree) else {
        return false;
    };
    packages.iter().all(|package| {
        npm_cache_digest_path(cache, &package.integrity).is_some_and(|path| {
            fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        })
    })
}

pub(super) fn decode_base64(value: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if value.len() != 88 || !value.ends_with("==") || value[..86].contains('=') {
        return None;
    }
    let mut out = Vec::with_capacity(64);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes().take(86) {
        accumulator = (accumulator << 6) | u32::from(sextet(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    (out.len() == 64 && accumulator & 0x0f == 0).then_some(out)
}
