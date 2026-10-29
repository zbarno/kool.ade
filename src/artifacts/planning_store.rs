//! Explicit owner for planning artifacts.
//!
//! A [`PlanningStore`] is always the standalone Git repository that holds a
//! project's planning state. Planning state never lives inside a code
//! checkout. See `docs/planning-repository-v1.md`.
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
    /// Temporary migration-reader mode: points INTO a legacy
    /// `.koolade-packet/` directory inside a source checkout. Only the
    /// explicit managed-workspace migration flow may construct stores this
    /// way; connected projects never run in this mode.
    LegacyPacketSource,
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
        Self {
            project_id,
            root,
            mode,
        }
    }

    /// Migration-exception constructor: opens the legacy
    /// `.koolade-packet/` directory of a source checkout being migrated into
    /// a managed workspace. Outside the explicit migration flow, checkouts
    /// do not hold planning state and this constructor must not be used.
    pub fn legacy_embedded(project_id: uuid::Uuid, repository: &Path) -> Self {
        Self::new(
            project_id,
            repository.join(crate::artifacts::layout::store::ROOT),
            StoreMode::LegacyPacketSource,
        )
    }

    pub fn layout(&self) -> PlanningLayout {
        PlanningLayout::for_store(self)
    }

    /// Git root for checkpointing planning writes. Stores commit at their
    /// own repository root; the migration reader resolves to the source
    /// checkout for compatibility with pre-migration checkpoints.
    pub fn git_root(&self) -> PathBuf {
        match self.mode {
            StoreMode::LegacyPacketSource => self.root.parent().unwrap_or(&self.root).to_path_buf(),
            StoreMode::ManagedLocal | StoreMode::ManagedShared => self.root.clone(),
        }
    }

    /// Convert a store-relative path to the path staged by the store's Git
    /// repository.
    pub fn git_path(&self, relative: &str) -> String {
        match self.mode {
            StoreMode::LegacyPacketSource => {
                format!("{}/{}", crate::artifacts::layout::store::ROOT, relative)
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
