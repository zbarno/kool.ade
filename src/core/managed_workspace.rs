//! Local workspace identity and its external planning Git repository.

mod association;
mod creation;
mod git;
mod locking;
mod manager;
mod manifest;
mod repositories;
#[cfg(test)]
mod tests;

pub use association::{CodeCheckout, WorkspaceAssociation};
#[cfg(test)]
pub(crate) use creation::CreationFailurePoint;
pub use manager::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager};
pub use manifest::WorkspaceManifest;

/// Resolve a directory inside a Git working tree to its actual repository
/// root. Git environment overrides are cleared by the subprocess helper.
pub fn repository_root(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    git::repository_root(path)
}
