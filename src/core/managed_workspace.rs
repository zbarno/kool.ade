//! Local workspace identity and its external planning Git repository.

mod application;
mod association;
mod existing_project;
pub use application::ApplicationCreation;
pub use existing_project::{ExistingProject, ExistingProjectKind};
mod creation;
mod git;
mod locking;
mod manager;
mod manifest;
mod migration;
mod recency;
mod recovery;
pub use recovery::{ArtifactPresence, RecoveryItem, RecoveryRequest};
mod pair;
mod repositories;
#[cfg(test)]
mod tests;

pub use association::{CodeCheckout, WorkspaceAssociation};
#[cfg(test)]
pub(crate) use creation::CreationFailurePoint;
pub use manager::{
    CodeRepositoryRegistration, LocalWorkspaceSummary, ManagedWorkspace, WorkspaceLauncher,
    WorkspaceManager,
};
pub(crate) use manifest::MAX_MANIFEST_BYTES;
pub use manifest::WorkspaceManifest;
pub use migration::{MigrationConflictPreview, MigrationPreview, MigrationReport, RollbackOutcome};
pub use pair::PAIR_CODE_REPO_ID;
pub use pair::SiblingPairSpec;
pub(crate) use pair::repository_name_allowed;

/// Resolve a directory inside a Git working tree to its actual repository
/// root. Git environment overrides are cleared by the subprocess helper.
pub fn repository_root(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    git::repository_root(path)
}
