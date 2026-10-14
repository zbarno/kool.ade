use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};
use tar::Archive;

mod capture;
pub(crate) use capture::capture;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fingerprint {
    Directory,
    File {
        sha256: String,
        size: u64,
        mode: u32,
    },
}

pub(crate) type Manifest = BTreeMap<PathBuf, Fingerprint>;

#[derive(Debug)]
pub(super) struct ReturnedEntry {
    pub fingerprint: Fingerprint,
    pub bytes: Vec<u8>,
    pub mode: u32,
}

pub(super) fn parse_returned(
    bytes: &[u8],
    byte_limit: u64,
    entry_limit: usize,
) -> anyhow::Result<BTreeMap<PathBuf, ReturnedEntry>> {
    anyhow::ensure!(
        bytes.len() as u64 <= byte_limit,
        "returned archive exceeds sync budget"
    );
    let mut archive = Archive::new(Cursor::new(bytes));
    let mut entries = BTreeMap::<PathBuf, ReturnedEntry>::new();
    let mut total = 0_u64;
    for entry in archive.entries()? {
        let entry = entry?;
        let raw_path = entry.path()?.into_owned();
        // `tar -C /workspace .` prefixes entries with `./`. Normalize that
        // conventional prefix before applying the stricter workspace checks.
        let path = raw_path
            .strip_prefix("./")
            .unwrap_or(&raw_path)
            .to_path_buf();
        if path.as_os_str().is_empty() || path == Path::new(".") {
            continue;
        }
        validate_relative(&path)?;
        if path
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            continue;
        }
        anyhow::ensure!(
            entries.len() < entry_limit,
            "returned archive has too many entries"
        );
        let kind = entry.header().entry_type();
        let mode = entry.header().mode().unwrap_or(0o644) & 0o777;
        let returned = if kind.is_dir() {
            ReturnedEntry {
                fingerprint: Fingerprint::Directory,
                bytes: Vec::new(),
                mode,
            }
        } else if kind.is_file() {
            let size = entry.header().size()?;
            total = total.saturating_add(size);
            anyhow::ensure!(total <= byte_limit, "returned files exceed sync budget");
            let mut content = Vec::with_capacity(size.min(1024 * 1024) as usize);
            entry
                .take(
                    byte_limit
                        .saturating_sub(total)
                        .saturating_add(size)
                        .saturating_add(1),
                )
                .read_to_end(&mut content)?;
            anyhow::ensure!(
                content.len() as u64 == size,
                "returned file size does not match archive header"
            );
            let fingerprint = Fingerprint::File {
                sha256: hash(&content),
                size,
                mode,
            };
            ReturnedEntry {
                fingerprint,
                bytes: content,
                mode,
            }
        } else if kind.is_hard_link() {
            let raw_target = entry
                .link_name()?
                .ok_or_else(|| anyhow::anyhow!("returned hard link has no target"))?
                .into_owned();
            let target = raw_target
                .strip_prefix("./")
                .unwrap_or(&raw_target)
                .to_path_buf();
            validate_relative(&target)?;
            if target
                .components()
                .any(|component| component.as_os_str() == ".git")
            {
                continue;
            }
            let source = entries
                .get(&target)
                .ok_or_else(|| anyhow::anyhow!("returned hard link target is unavailable"))?;
            anyhow::ensure!(
                matches!(&source.fingerprint, Fingerprint::File { .. }),
                "returned hard link target is not a regular file"
            );
            let content = source.bytes.clone();
            total = total.saturating_add(content.len() as u64);
            anyhow::ensure!(total <= byte_limit, "returned files exceed sync budget");
            let fingerprint = Fingerprint::File {
                sha256: hash(&content),
                size: content.len() as u64,
                mode,
            };
            ReturnedEntry {
                fingerprint,
                bytes: content,
                mode,
            }
        } else {
            anyhow::bail!("returned archive contains an unsupported entry type");
        };
        anyhow::ensure!(
            entries.insert(path, returned).is_none(),
            "returned archive contains duplicate paths"
        );
    }
    // Reject file/child collisions and materialize implicit parent directories
    // so archive ordering and omitted directory headers cannot redirect writes.
    let paths = entries.keys().cloned().collect::<Vec<_>>();
    for path in paths {
        let mut parent = path.parent();
        while let Some(parent_path) = parent {
            if parent_path.as_os_str().is_empty() {
                break;
            }
            match entries.get(parent_path) {
                Some(ReturnedEntry {
                    fingerprint: Fingerprint::File { .. },
                    ..
                }) => {
                    anyhow::bail!("returned archive has a file as a parent directory");
                }
                Some(_) => {}
                None => {
                    anyhow::ensure!(
                        entries.len() < entry_limit,
                        "returned archive has too many entries"
                    );
                    entries.insert(
                        parent_path.to_path_buf(),
                        ReturnedEntry {
                            fingerprint: Fingerprint::Directory,
                            bytes: Vec::new(),
                            mode: 0o755,
                        },
                    );
                }
            }
            parent = parent_path.parent();
        }
    }
    Ok(entries)
}

pub(super) fn validate_relative(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(!path.as_os_str().is_empty(), "archive path is empty");
    anyhow::ensure!(path.as_os_str().len() <= 4096, "archive path is too long");
    for component in path.components() {
        anyhow::ensure!(
            matches!(component, Component::Normal(_)),
            "archive path escapes workspace"
        );
    }
    anyhow::ensure!(path.to_str().is_some(), "archive path is not UTF-8");
    Ok(())
}

pub(super) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
