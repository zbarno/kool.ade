use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::denied;
use crate::execution::errors::ExecutionError;

mod selected;
pub use selected::snapshot_selected_files;

#[derive(Clone, Debug)]
pub struct QaArtifactMount {
    source: PathBuf,
    target: String,
}

#[derive(Clone, Debug)]
pub struct QaContainerUpload {
    pub target: String,
    pub archive: Vec<u8>,
}

struct Entry {
    name: String,
    bytes: Option<Vec<u8>>,
    executable: bool,
}

impl QaArtifactMount {
    /// Select an app-owned artifact directory that will be snapshotted and
    /// uploaded into container tmpfs before the QA driver starts.
    pub fn inspect(source: &Path, target: &str) -> Result<Self, ExecutionError> {
        if !matches!(target, "/workspace" | "/run/koolade-runtime-qa") {
            return Err(denied("Runtime QA artifact mount target is not approved"));
        }
        let metadata = fs::symlink_metadata(source)
            .map_err(|_| denied("Runtime QA artifact directory cannot be inspected"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(denied(
                "Runtime QA artifact directory must be a real directory",
            ));
        }
        let canonical = source
            .canonicalize()
            .map_err(|_| denied("Runtime QA artifact directory is unavailable"))?;
        Ok(Self {
            source: canonical,
            target: target.into(),
        })
    }

    /// Capture an immutable tar snapshot. The returned digest is over sorted
    /// relative paths and bytes, independent of host metadata and timestamps.
    pub fn snapshot_upload(
        &self,
        maximum_bytes: usize,
        expected_digest: Option<&str>,
    ) -> Result<(QaContainerUpload, String), ExecutionError> {
        if maximum_bytes == 0 {
            return Err(denied("Runtime QA artifact snapshot byte bound is empty"));
        }
        let mut entries = Vec::new();
        collect_entries(&self.source, &self.source, &mut entries)?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let mut digest = Sha256::new();
        let mut total = 0usize;
        let mut archive = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut archive);
            for entry in entries {
                digest.update(entry.name.as_bytes());
                digest.update([0]);
                let mut header = tar::Header::new_gnu();
                if let Some(bytes) = entry.bytes {
                    total = total.saturating_add(bytes.len());
                    if total > maximum_bytes {
                        return Err(denied(
                            "Runtime QA artifact snapshot exceeds its byte bound",
                        ));
                    }
                    digest.update(&bytes);
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_size(bytes.len() as u64);
                    header.set_mode(if entry.executable { 0o555 } else { 0o444 });
                    header.set_cksum();
                    tar.append_data(&mut header, &entry.name, bytes.as_slice())
                        .map_err(|_| denied("Runtime QA artifact snapshot could not be encoded"))?;
                } else {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o555);
                    header.set_cksum();
                    tar.append_data(&mut header, &entry.name, std::io::empty())
                        .map_err(|_| denied("Runtime QA artifact snapshot could not be encoded"))?;
                }
            }
            tar.finish()
                .map_err(|_| denied("Runtime QA artifact snapshot could not be finalized"))?;
        }
        let digest = format!("{:x}", digest.finalize());
        if expected_digest.is_some_and(|expected| expected != digest) {
            return Err(denied(
                "Runtime QA artifact snapshot does not match its approved digest",
            ));
        }
        Ok((
            QaContainerUpload {
                target: self.target.clone(),
                archive,
            },
            digest,
        ))
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

impl QaContainerUpload {
    pub fn request_file(bytes: &[u8], maximum_bytes: usize) -> Result<Self, ExecutionError> {
        if bytes.is_empty() || bytes.len() > maximum_bytes {
            return Err(denied("Runtime QA request exceeds its upload byte bound"));
        }
        let mut archive = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut archive);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(0o444);
            header.set_cksum();
            tar.append_data(&mut header, "request.json", bytes)
                .map_err(|_| denied("Runtime QA request archive could not be encoded"))?;
            tar.finish()
                .map_err(|_| denied("Runtime QA request archive could not be finalized"))?;
        }
        Ok(Self {
            target: "/run/koolade-runtime-qa".into(),
            archive,
        })
    }
}

fn collect_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<Entry>,
) -> Result<(), ExecutionError> {
    let mut children = fs::read_dir(directory)
        .map_err(|_| denied("Runtime QA artifact directory cannot be read"))?
        .map(|entry| entry.map_err(|_| denied("Runtime QA artifact entry cannot be read")))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| denied("Runtime QA artifact metadata cannot be read"))?;
        if metadata.file_type().is_symlink() {
            return Err(denied("Runtime QA artifacts cannot contain symlinks"));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| denied("Runtime QA artifact escaped its selected root"))?;
        let name = relative
            .components()
            .map(|part| match part {
                std::path::Component::Normal(value) => value
                    .to_str()
                    .ok_or_else(|| denied("Runtime QA artifact path is not valid UTF-8")),
                _ => Err(denied("Runtime QA artifact path contains traversal")),
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        if metadata.is_dir() {
            entries.push(Entry {
                name: format!("{name}/"),
                bytes: None,
                executable: false,
            });
            collect_entries(root, &path, entries)?;
        } else if metadata.is_file() {
            let mut file = fs::File::open(&path)
                .map_err(|_| denied("Runtime QA artifact file could not be opened"))?;
            let opened = file
                .metadata()
                .map_err(|_| denied("Runtime QA artifact file metadata is unavailable"))?;
            if !opened.is_file() || opened.len() != metadata.len() {
                return Err(denied(
                    "Runtime QA artifact changed while it was snapshotted",
                ));
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|_| denied("Runtime QA artifact file could not be captured"))?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            entries.push(Entry {
                name,
                bytes: Some(bytes),
                executable,
            });
        } else {
            return Err(denied("Runtime QA artifact contains a special file"));
        }
    }
    Ok(())
}
