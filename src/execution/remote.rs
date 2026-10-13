//! Authenticated remote-worker transport primitives.
//!
//! SSH identity and worker authorization are separate from framing; valid
//! frames and transfer manifests do not grant execution authority.

pub mod backend;
pub mod client;
pub mod dependency;
pub mod protocol;
pub mod scheduler;
pub mod server;
pub mod ssh;
pub mod staging;
pub mod transport;
pub mod worker;
mod worker_capacity;
pub(crate) use worker_capacity::{WorkerCapacityLease, WorkerCapacityPool};
mod workspace;

pub use workspace::{WorkspaceEntryKindV1, WorkspaceEntryV1, WorkspaceManifestV1};
