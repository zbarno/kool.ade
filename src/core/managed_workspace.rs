//! Local workspace identity and its external planning Git repository.

mod association;
mod creation;
mod git;
mod locking;
mod manager;
mod manifest;
mod migration;
mod pair;
mod repositories;
#[cfg(test)]
mod tests;

pub use association::{CodeCheckout, WorkspaceAssociation};
#[cfg(test)]
pub(crate) use creation::CreationFailurePoint;
pub use manager::{
    CodeRepositoryRegistration, LocalWorkspaceSummary, ManagedWorkspace, WorkspaceManager,
};
pub(crate) use manifest::MAX_MANIFEST_BYTES;
pub use manifest::WorkspaceManifest;
pub use migration::{MigrationConflictPreview, MigrationPreview, MigrationReport, RollbackOutcome};
pub use pair::SiblingPairSpec;

/// Resolve a directory inside a Git working tree to its actual repository
/// root. Git environment overrides are cleared by the subprocess helper.
pub fn repository_root(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    git::repository_root(path)
}
