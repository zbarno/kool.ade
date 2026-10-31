//! Explicit owner for planning artifacts.
//!
//! A [`PlanningStore`] is always the standalone Git repository that holds a
//! project's planning state. Planning state never lives inside a code
//! checkout. See `docs/planning-repository-v1.md`.
#[cfg(test)]
use std::{
    collections::HashSet,
    sync::{OnceLock, RwLock},
};
use std::{
    fmt, io,
    path::{Path, PathBuf},
};

mod layout;
mod record_api;
mod records;
mod root;
#[path = "planning_store/io.rs"]
mod store_io;
#[cfg(test)]
mod tests;

pub use layout::{PlanningLayout, paths};
pub use records::RecordRevisionCheck;
pub(crate) use records::validate_record_revisions as validate_record_changes;
pub use root::PlanningRoot;

pub(crate) type RecordChangePlan = (Vec<(String, Vec<u8>)>, Vec<RecordRevisionCheck>, bool);

/// Local-only slots: private state (MCP operator config, implementation
/// journal, time ledger) that must never enter the shared planning-store
/// sync. Defined ONCE so the store I/O fence and the transaction fence
/// cannot drift apart.
pub(crate) fn is_local_only(relative: &Path) -> bool {
    relative == Path::new("config/mcp.json")
        || relative.starts_with("implementation")
        || relative == Path::new("state/time-ledger.log")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    ManagedLocal,
    ManagedShared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningStore {
    pub project_id: uuid::Uuid,
    pub root: PathBuf,
    pub mode: StoreMode,
    #[cfg(test)]
    pub(crate) revision_code_root_fixture: bool,
}

#[cfg(test)]
static CODE_ROOT_FIXTURES: OnceLock<RwLock<HashSet<PathBuf>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFile {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum StoreError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidPath(String),
    StaleRevision {
        expected: String,
        actual: String,
    },
    StaleRecordRevision {
        path: String,
        expected: u64,
        actual: u64,
    },
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
            Self::StaleRecordRevision {
                path,
                expected,
                actual,
            } => write!(
                f,
                "planning record {path} changed (expected revision {expected}, found {actual})"
            ),
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
        #[cfg(test)]
        let revision_code_root_fixture = is_code_root_fixture(&root);
        Self {
            project_id,
            root,
            mode,
            #[cfg(test)]
            revision_code_root_fixture,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_code_root_fixture(
        project_id: uuid::Uuid,
        root: impl Into<PathBuf>,
        mode: StoreMode,
    ) -> Self {
        let mut store = Self::new(project_id, root, mode);
        CODE_ROOT_FIXTURES
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(store.root.clone());
        store.revision_code_root_fixture = true;
        store
    }

    pub fn layout(&self) -> PlanningLayout {
        PlanningLayout::for_store(self)
    }

    /// Git root for checkpointing planning writes.
    pub fn git_root(&self) -> PathBuf {
        self.root.clone()
    }

    /// Convert a store-relative path to the path staged by the store's Git
    /// repository.
    pub fn git_path(&self, relative: &str) -> String {
        relative.to_owned()
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

    /// Atomically write independent record files after checking each record's
    /// own revision. Changes to unrelated records do not invalidate these
    /// writes; a stale same-record update is rejected.
    pub fn transaction_with_record_revisions(
        &self,
        changes: &[(String, Vec<u8>)],
        expected_records: &[RecordRevisionCheck],
    ) -> Result<(Vec<String>, String), StoreError> {
        crate::artifacts::transaction::apply_store_with_record_revisions(
            self,
            changes,
            expected_records,
        )
    }

    /// Combine a store-wide snapshot fence with record-specific checks for a
    /// transaction that also changes legacy singleton planning artifacts.
    pub fn transaction_with_revision_and_record_revisions(
        &self,
        changes: &[(String, Vec<u8>)],
        expected_revision: Option<&str>,
        expected_records: &[RecordRevisionCheck],
    ) -> Result<(Vec<String>, String), StoreError> {
        crate::artifacts::transaction::apply_store_with_revision_and_record_revisions(
            self,
            changes,
            expected_revision,
            expected_records,
        )
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

    pub fn transaction_with_removals_and_record_revisions(
        &self,
        changes: &[(String, Vec<u8>)],
        removals: &[String],
        expected_revision: Option<&str>,
        expected_records: &[RecordRevisionCheck],
    ) -> Result<(Vec<String>, String), StoreError> {
        crate::artifacts::transaction::apply_store_with_removals_and_record_revisions(
            self,
            changes,
            removals,
            expected_revision,
            expected_records,
        )
    }

    pub fn revision(&self) -> Result<String, StoreError> {
        crate::artifacts::transaction::revision(self)
    }

    pub fn recover(&self) -> Result<bool, StoreError> {
        self.validate_root()?;
        crate::artifacts::transaction::recover_store(self)
    }

    pub(crate) fn with_consistent_read<T, E>(
        &self,
        read: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<StoreError>,
    {
        self.validate_root().map_err(E::from)?;
        crate::artifacts::transaction::with_consistent_read(self, read)
    }
}

#[cfg(test)]
fn is_code_root_fixture(root: &Path) -> bool {
    CODE_ROOT_FIXTURES
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(root)
}
