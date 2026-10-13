//! Bounded, content-addressed package cache snapshots for controller-to-worker transfer.
//!
//! Snapshots contain only files below an app-owned broker cache root. They never serialize a
//! host path; consumers verify an archive and manifest before materializing task-scoped caches.

mod capture;
mod manifest;

#[cfg(test)]
mod tests;

pub(crate) use crate::execution::contract::DependencyCacheRefV1 as DependencyCacheRef;
pub(crate) use capture::CacheSnapshot;
pub(crate) use manifest::CacheSnapshotManifest;
pub(crate) const MAX_CACHE_SNAPSHOT_BYTES: u64 = 128 * 1024 * 1024;
