//! Private, content-verified handoff for broker cache snapshots between task
//! execution and a later isolated Runtime QA session.

use crate::harness::resource_bridge::{CacheSnapshot, CacheSnapshotManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf};

const MAX_STORED_SNAPSHOTS: usize = 512;
const MAX_STORED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_STORED_HEADER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DependencyCacheSnapshotStore {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotHeader {
    schema_version: u16,
    dependency_request_id: String,
    cache_ref: crate::execution::contract::DependencyCacheRefV1,
    manifest: CacheSnapshotManifest,
}

impl DependencyCacheSnapshotStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn put(
        &self,
        dependency_request_id: &str,
        snapshot: &CacheSnapshot,
    ) -> anyhow::Result<()> {
        crate::execution::contract::validate_identifier(dependency_request_id)?;
        snapshot
            .manifest
            .validate_ref(&snapshot.cache_ref, &snapshot.archive)?;
        self.ensure_private_root()?;
        let lock_path = self.root.join(".lock");
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        set_private_permissions(&lock_path)?;
        fs2::FileExt::lock_exclusive(&lock)?;
        let result = self.put_locked(dependency_request_id, snapshot);
        let unlock_result = fs2::FileExt::unlock(&lock);
        result?;
        unlock_result?;
        Ok(())
    }

    fn put_locked(
        &self,
        dependency_request_id: &str,
        snapshot: &CacheSnapshot,
    ) -> anyhow::Result<()> {
        let destination = self.snapshot_dir(dependency_request_id, &snapshot.cache_ref)?;
        if destination.exists() {
            let existing = self.get(dependency_request_id, &snapshot.cache_ref)?;
            anyhow::ensure!(
                existing.archive == snapshot.archive,
                "cache snapshot is immutable"
            );
            return Ok(());
        }
        let header = SnapshotHeader {
            schema_version: 1,
            dependency_request_id: dependency_request_id.to_owned(),
            cache_ref: snapshot.cache_ref.clone(),
            manifest: snapshot.manifest.clone(),
        };
        let header_bytes = serde_json::to_vec(&header)?;
        anyhow::ensure!(
            header_bytes.len() <= MAX_STORED_HEADER_BYTES,
            "cache snapshot metadata exceeds its size limit"
        );
        self.ensure_capacity_for(snapshot.archive.len(), header_bytes.len())?;

        let temporary = self.root.join(format!(
            ".staging-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        create_private_dir(&temporary)?;
        let result = (|| {
            write_private_file(&temporary.join("manifest.json"), &header_bytes)?;
            write_private_file(&temporary.join("snapshot.tar"), &snapshot.archive)?;
            match fs::rename(&temporary, &destination) {
                Ok(()) => Ok(()),
                Err(_) if destination.exists() => {
                    let existing = self.get(dependency_request_id, &snapshot.cache_ref)?;
                    anyhow::ensure!(
                        existing.archive == snapshot.archive,
                        "cache snapshot is immutable"
                    );
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        })();
        let _ = fs::remove_dir_all(&temporary);
        result
    }

    fn ensure_capacity_for(
        &self,
        incoming_archive_bytes: usize,
        incoming_header_bytes: usize,
    ) -> anyhow::Result<()> {
        let mut count = 0usize;
        let mut total_bytes = 0u64;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".lock" {
                continue;
            }
            if name.starts_with(".staging-") {
                fs::remove_dir_all(entry.path())?;
                continue;
            }
            ensure_real_directory(&entry.path())?;
            count = count.saturating_add(1);
            for filename in ["manifest.json", "snapshot.tar"] {
                let path = entry.path().join(filename);
                let metadata = fs::symlink_metadata(&path)?;
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "cache store contains a non-regular snapshot file"
                );
                total_bytes = total_bytes.saturating_add(metadata.len());
            }
        }
        anyhow::ensure!(
            count < MAX_STORED_SNAPSHOTS
                && total_bytes
                    .saturating_add(incoming_archive_bytes as u64)
                    .saturating_add(incoming_header_bytes as u64)
                    <= MAX_STORED_BYTES,
            "dependency cache store capacity is reached; remove old task cache snapshots"
        );
        Ok(())
    }

    pub(crate) fn get(
        &self,
        dependency_request_id: &str,
        expected_ref: &crate::execution::contract::DependencyCacheRefV1,
    ) -> anyhow::Result<CacheSnapshot> {
        crate::execution::contract::validate_identifier(dependency_request_id)?;
        expected_ref.validate()?;
        self.ensure_private_root()?;
        let directory = self.snapshot_dir(dependency_request_id, expected_ref)?;
        ensure_real_directory(&directory)?;
        let header_bytes =
            read_regular_file(&directory.join("manifest.json"), MAX_STORED_HEADER_BYTES)?;
        let header: SnapshotHeader = serde_json::from_slice(&header_bytes)?;
        anyhow::ensure!(
            header.schema_version == 1
                && header.dependency_request_id == dependency_request_id
                && header.cache_ref == *expected_ref,
            "stored cache snapshot identity does not match the requested dependency"
        );
        let archive = read_regular_file(
            &directory.join("snapshot.tar"),
            crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES as usize,
        )?;
        header.manifest.validate_ref(expected_ref, &archive)?;
        Ok(CacheSnapshot {
            cache_ref: header.cache_ref,
            manifest: header.manifest,
            archive,
        })
    }

    fn snapshot_dir(
        &self,
        dependency_request_id: &str,
        cache_ref: &crate::execution::contract::DependencyCacheRefV1,
    ) -> anyhow::Result<PathBuf> {
        let identity = serde_json::to_vec(&(dependency_request_id, cache_ref))?;
        let name = format!("{:x}", Sha256::digest(identity));
        Ok(self.root.join(name))
    }

    fn ensure_private_root(&self) -> anyhow::Result<()> {
        if self.root.exists() {
            ensure_real_directory(&self.root)?;
            set_private_permissions(&self.root)?;
        } else {
            fs::create_dir_all(&self.root)?;
            set_private_permissions(&self.root)?;
        }
        Ok(())
    }
}

fn ensure_real_directory(path: &std::path::Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "cache store path is not a real directory"
    );
    Ok(())
}

fn create_private_dir(path: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    set_private_permissions(path)
}

fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    set_private_permissions(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_regular_file(path: &std::path::Path, limit: usize) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit as u64,
        "cache snapshot file is not regular or exceeds its size limit"
    );
    Ok(fs::read(path)?)
}

#[cfg(unix)]
fn set_private_permissions(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
#[path = "dependency_cache_store/tests.rs"]
mod tests;
