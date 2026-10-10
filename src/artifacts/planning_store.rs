//! Explicit owner for shared planning artifacts.
//!
//! Legacy projects keep their existing `.koolade-packet` files in place. A
//! managed planning repository can use the same API with its own root.
use std::{
    fmt, io,
    path::{Path, PathBuf},
};

mod layout;
mod root;
#[path = "planning_store/io.rs"]
mod store_io;
#[cfg(test)]
mod tests;

pub use layout::{PlanningLayout, paths};
pub use root::PlanningRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    LegacyEmbedded,
    ManagedLocal,
    ManagedShared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningStore {
    pub project_id: uuid::Uuid,
    pub root: PathBuf,
    pub mode: StoreMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFile {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum StoreError {
    Io { path: PathBuf, source: io::Error },
    InvalidPath(String),
    StaleRevision { expected: String, actual: String },
    MalformedState(String),
    UnavailableStore(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::InvalidPath(path) => write!(f, "invalid planning path: {path}"),
            Self::StaleRevision { expected, actual } => {
                write!(
                    f,
                    "planning store changed (expected {expected}, found {actual})"
                )
            }
            Self::MalformedState(detail) => write!(f, "malformed planning state: {detail}"),
            Self::UnavailableStore(detail) => write!(f, "planning store unavailable: {detail}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl PlanningStore {
    pub fn new(project_id: uuid::Uuid, root: impl Into<PathBuf>, mode: StoreMode) -> Self {
        let root = root.into();
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map(|current| current.join(&root))
                .unwrap_or(root)
        };
        let root_is_symlink = std::fs::symlink_metadata(&root)
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        let root = if root_is_symlink {
            root
        } else {
            std::fs::canonicalize(&root).unwrap_or(root)
        };
        Self {
            project_id,
            root,
            mode,
        }
    }

    /// Compatibility mapping. Opening a legacy project never copies or moves
    /// its existing artifacts.
    pub fn legacy_embedded(project_id: uuid::Uuid, repository: &Path) -> Self {
        Self::new(
            project_id,
            repository.join(crate::artifacts::layout::canonical::ROOT),
            StoreMode::LegacyEmbedded,
        )
    }

    pub fn layout(&self) -> PlanningLayout {
        PlanningLayout::for_store(self)
    }

    /// Git root for checkpointing planning writes. In embedded mode this is
    /// the code checkout for compatibility; managed stores commit at their
    /// own root.
    pub fn git_root(&self) -> PathBuf {
        match self.mode {
            StoreMode::LegacyEmbedded => self.root.parent().unwrap_or(&self.root).to_path_buf(),
            StoreMode::ManagedLocal | StoreMode::ManagedShared => self.root.clone(),
        }
    }

    /// Convert a store-relative path to the path staged by the store's Git
    /// repository.
    pub fn git_path(&self, relative: &str) -> String {
        match self.mode {
            StoreMode::LegacyEmbedded => {
                format!("{}/{relative}", crate::artifacts::layout::canonical::ROOT)
            }
            StoreMode::ManagedLocal | StoreMode::ManagedShared => relative.to_owned(),
        }
    }

    pub fn transaction(
        &self,
        changes: &[(String, Vec<u8>)],
        expected_revision: Option<&str>,
    ) -> Result<Vec<String>, StoreError> {
        crate::artifacts::transaction::apply_store(self, changes, expected_revision)
    }

    pub fn transaction_with_revision(
        &self,
        changes: &[(String, Vec<u8>)],
        expected_revision: Option<&str>,
    ) -> Result<(Vec<String>, String), StoreError> {
        crate::artifacts::transaction::apply_store_with_revision(self, changes, expected_revision)
    }

    pub fn transaction_with_removals(
        &self,
        changes: &[(String, Vec<u8>)],
        removals: &[String],
        expected_revision: Option<&str>,
    ) -> Result<Vec<String>, StoreError> {
        crate::artifacts::transaction::apply_store_with_removals(
            self,
            changes,
            removals,
            expected_revision,
        )
    }

    pub fn transaction_with_removals_and_revision(
        &self,
        changes: &[(String, Vec<u8>)],
        removals: &[String],
        expected_revision: Option<&str>,
    ) -> Result<(Vec<String>, String), StoreError> {
        crate::artifacts::transaction::apply_store_with_removals_and_revision(
            self,
            changes,
            removals,
            expected_revision,
        )
    }

    pub fn revision(&self) -> Result<String, StoreError> {
        crate::artifacts::transaction::revision(self)
    }

    pub fn recover(&self) -> Result<bool, StoreError> {
        self.validate_root()?;
        crate::artifacts::transaction::recover_store(self)
    }
}
