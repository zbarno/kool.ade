//! Versioned contracts for the future Docker-backed local and remote executors.
//! This module defines interfaces only; it does not start containers.

pub mod backend;
pub mod contract;
mod dependency_cache_store;
pub mod docker;
pub mod errors;
pub mod policy;
pub mod remote;
pub mod session;

pub(crate) use dependency_cache_store::DependencyCacheSnapshotStore;

#[cfg(test)]
#[path = "execution/tests.rs"]
mod tests;
