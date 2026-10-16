use super::manifest::{CacheSnapshotEntry, CacheSnapshotManifest, hex, validate_relative};
use crate::execution::contract::DependencyCacheEcosystemV1;
use crate::harness::resource_bridge::cache_snapshot::DependencyCacheRef;
use sha2::{Digest, Sha256};
use std::path::Path;

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod unix;

const MAX_ENTRIES: usize = 4_096;

#[derive(Clone, Debug)]
pub(crate) struct CacheSnapshot {
    pub manifest: CacheSnapshotManifest,
    pub cache_ref: DependencyCacheRef,
    pub archive: Vec<u8>,
}

impl CacheSnapshot {
    /// Capture an immutable archive from an app-owned cache directory.
    pub fn capture(
        root: &Path,
        task_id: &str,
        ecosystem: DependencyCacheEcosystemV1,
        project_id: &str,
        resolved_integrity: &str,
        target_arch: &str,
        target_id: &str,
    ) -> anyhow::Result<Self> {
        for identity in [
            task_id,
            project_id,
            target_arch,
            target_id,
            resolved_integrity,
        ] {
            anyhow::ensure!(
                !identity.is_empty()
                    && identity.len() <= 512
                    && !identity.bytes().any(|b| b.is_ascii_control()),
                "cache snapshot identity is invalid"
            );
        }
        let root = root.canonicalize()?;
        anyhow::ensure!(root.is_dir(), "cache snapshot root is not a directory");
        let mut files = capture_files_platform(&root)?;
        anyhow::ensure!(
            !files.is_empty() && files.len() <= MAX_ENTRIES,
            "cache snapshot entry count is invalid"
        );
        files.sort_by(|left, right| left.0.cmp(&right.0));

        let mut entries = Vec::with_capacity(files.len());
        let mut payloads = Vec::with_capacity(files.len());
        let mut total_bytes = 0_u64;
        for (relative, bytes) in files {
            validate_relative(&relative)?;
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            anyhow::ensure!(
                total_bytes <= super::super::MAX_CACHE_SNAPSHOT_BYTES,
                "cache snapshot exceeds the transfer limit"
            );
            entries.push(CacheSnapshotEntry {
                path: relative,
                size_bytes: bytes.len() as u64,
                sha256: hex(&Sha256::digest(&bytes)),
            });
            payloads.push(bytes);
        }
        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            builder.mode(tar::HeaderMode::Deterministic);
            for (entry, bytes) in entries.iter().zip(payloads) {
                let mut header = tar::Header::new_gnu();
                header.set_size(entry.size_bytes);
                header.set_mode(0o600);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                header.set_cksum();
                builder.append_data(&mut header, &entry.path, bytes.as_slice())?;
            }
            builder.finish()?;
        }
        anyhow::ensure!(
            archive.len() as u64 <= super::super::MAX_CACHE_SNAPSHOT_BYTES,
            "cache archive exceeds the transfer limit"
        );
        let manifest = CacheSnapshotManifest {
            schema_version: 1,
            task_id: task_id.to_owned(),
            ecosystem,
            project_id: project_id.to_owned(),
            resolved_integrity: resolved_integrity.to_owned(),
            target_arch: target_arch.to_owned(),
            target_id: target_id.to_owned(),
            total_bytes,
            archive_sha256: hex(&Sha256::digest(&archive)),
            entries,
        };
        manifest.validate(&archive)?;
        let manifest_sha256 = hex(&Sha256::digest(serde_json::to_vec(&manifest)?));
        let cache_ref = DependencyCacheRef {
            ecosystem,
            task_id: task_id.to_owned(),
            project_id: project_id.to_owned(),
            resolved_integrity: resolved_integrity.to_owned(),
            target_arch: target_arch.to_owned(),
            target_id: target_id.to_owned(),
            manifest_sha256,
            size_bytes: archive.len() as u64,
        };
        cache_ref.validate()?;
        Ok(Self {
            manifest,
            cache_ref,
            archive,
        })
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn capture_files_platform(root: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    unix::capture_files(root, super::super::MAX_CACHE_SNAPSHOT_BYTES, MAX_ENTRIES)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn capture_files_platform(_root: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    anyhow::bail!("safe cache snapshot capture is unavailable on this platform")
}
