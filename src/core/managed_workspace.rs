//! Local workspace identity and its external planning Git repository.

mod application;
mod association;
pub use application::ApplicationCreation;
pub(crate) use application::publish_directory;
mod creation;
mod git;
mod locking;
mod manager;
mod manifest;
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
pub use pair::NewProjectSpec;
pub use pair::PAIR_CODE_REPO_ID;
pub(crate) use pair::repository_name_allowed;

/// Resolve a directory inside a Git working tree to its actual repository
/// root. Git environment overrides are cleared by the subprocess helper.
pub fn repository_root(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    git::repository_root(path)
}
