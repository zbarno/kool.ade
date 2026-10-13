use crate::execution::contract::DependencyCacheEcosystemV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Component, Path},
};

const MAX_ENTRIES: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheSnapshotEntry {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheSnapshotManifest {
    pub schema_version: u16,
    pub task_id: String,
    pub ecosystem: DependencyCacheEcosystemV1,
    pub project_id: String,
    pub resolved_integrity: String,
    pub target_arch: String,
    pub target_id: String,
    pub total_bytes: u64,
    pub archive_sha256: String,
    pub entries: Vec<CacheSnapshotEntry>,
}

impl CacheSnapshotManifest {
    pub fn validate_ref(
        &self,
        cache_ref: &crate::harness::resource_bridge::cache_snapshot::DependencyCacheRef,
        archive: &[u8],
    ) -> anyhow::Result<()> {
        cache_ref.validate()?;
        self.validate(archive)?;
        anyhow::ensure!(
            cache_ref.task_id == self.task_id
                && cache_ref.ecosystem == self.ecosystem
                && cache_ref.project_id == self.project_id
                && cache_ref.resolved_integrity == self.resolved_integrity
                && cache_ref.target_arch == self.target_arch
                && cache_ref.target_id == self.target_id
                && cache_ref.size_bytes == archive.len() as u64
                && cache_ref.manifest_sha256 == hex(&Sha256::digest(serde_json::to_vec(self)?)),
            "dependency cache reference does not match its snapshot"
        );
        Ok(())
    }

    pub fn validate(&self, archive: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported cache snapshot schema"
        );
        anyhow::ensure!(
            self.total_bytes <= super::MAX_CACHE_SNAPSHOT_BYTES
                && archive.len() as u64 <= super::MAX_CACHE_SNAPSHOT_BYTES,
            "cache snapshot exceeds the transfer limit"
        );
        anyhow::ensure!(
            hex(&Sha256::digest(archive)) == self.archive_sha256,
            "cache snapshot archive digest mismatch"
        );
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for entry in &self.entries {
            validate_relative(&entry.path)?;
            anyhow::ensure!(
                paths.insert(entry.path.as_str()),
                "cache snapshot has duplicate paths"
            );
            total = total.saturating_add(entry.size_bytes);
            anyhow::ensure!(
                entry.sha256.len() == 64 && entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "cache entry digest is invalid"
            );
        }
        anyhow::ensure!(
            total == self.total_bytes && self.entries.len() <= MAX_ENTRIES,
            "cache snapshot manifest totals do not match"
        );
        let mut tar = tar::Archive::new(archive);
        let mut observed = 0usize;
        for item in tar.entries()? {
            let item = item?;
            let path = item.path()?.to_string_lossy().into_owned();
            validate_relative(&path)?;
            let expected = self
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .ok_or_else(|| anyhow::anyhow!("cache archive contains an unlisted path"))?;
            anyhow::ensure!(
                item.header().entry_type().is_file() && item.size() == expected.size_bytes,
                "cache archive contains an invalid entry"
            );
            let mut bytes = Vec::with_capacity(
                expected.size_bytes.min(super::MAX_CACHE_SNAPSHOT_BYTES) as usize,
            );
            item.take(super::MAX_CACHE_SNAPSHOT_BYTES + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 == expected.size_bytes
                    && hex(&Sha256::digest(&bytes)) == expected.sha256,
                "cache entry digest mismatch"
            );
            observed += 1;
        }
        anyhow::ensure!(
            observed == self.entries.len(),
            "cache archive is incomplete"
        );
        Ok(())
    }
}

pub(super) fn validate_relative(value: &str) -> anyhow::Result<()> {
    let path = Path::new(value);
    anyhow::ensure!(
        !value.is_empty()
            && !value.contains('\\')
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "cache entry path is unsafe"
    );
    Ok(())
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
