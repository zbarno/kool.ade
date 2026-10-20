//! Authenticated remote-worker transport primitives.
//!
//! SSH identity and worker authorization are separate from framing; valid
//! frames and transfer manifests do not grant execution authority.

// Raw remote backends intentionally remain internal: constructing one bypasses
// the session-level claim verifier and authorization lifecycle.
pub(crate) mod backend;
// Raw RPC clients can issue mutating worker commands without passing through
// the claim-guarded ExecutionSession lifecycle.
pub(crate) mod client;
pub mod dependency;
pub(crate) mod private_files;
pub mod protocol;
pub mod scheduler;
pub(crate) mod server;
pub mod ssh;
pub mod staging;
// The SSH stdio transport is only safe behind the session-owned RPC client.
pub(crate) mod transport;
pub(crate) mod worker;
mod worker_capacity;
pub(crate) use worker_capacity::{WorkerCapacityLease, WorkerCapacityPool};
mod workspace;

pub use workspace::{WorkspaceEntryKindV1, WorkspaceEntryV1, WorkspaceManifestV1};

/// Serve the configured worker command over standard input and output.
/// The raw protocol and dispatch modules remain internal to the application.
pub fn serve_worker_stdio() -> anyhow::Result<()> {
    worker::serve_stdio_worker()
}
