//! Installation-local store for operational project data and private drafts.
//!
//! Published product, feature, and architecture documents are read from the
//! primary code repository by the pinned published-spec service. This store
//! remains outside every Git repository and is never synchronized.
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

pub(crate) type RecordChangePlan = (Vec<(String, Vec<u8>)>, Vec<RecordRevisionCheck>);

/// Private records managed by their own bounded local-storage services.
pub(crate) fn is_local_only(relative: &Path) -> bool {
    relative == Path::new("config/mcp.json")
        || relative.starts_with("implementation")
        || relative == Path::new("state/time-ledger.log")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    ManagedLocal,
}

#[derive(Debug, Clone)]
pub struct PlanningStore {
    pub project_id: uuid::Uuid,
    pub root: PathBuf,
    pub mode: StoreMode,
    /// Primary code repository used only for pinned published-document reads.
    pub published_repository_root: Option<PathBuf>,
    /// Optional immutable revision already selected for this loaded project.
    pub published_spec_pin: Option<crate::core::published_specs::PublishedSpecPin>,
}

impl PartialEq for PlanningStore {
    fn eq(&self, other: &Self) -> bool {
        self.project_id == other.project_id
            && self.root == other.root
            && self.mode == other.mode
            && self.published_repository_root == other.published_repository_root
            && self.published_spec_pin == other.published_spec_pin
    }
}

impl Eq for PlanningStore {}

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
            published_repository_root: None,
            published_spec_pin: None,
        }
    }

    pub fn for_code_repository(
        project_id: uuid::Uuid,
        root: impl Into<PathBuf>,
        code_repository: impl Into<PathBuf>,
    ) -> Self {
        let mut store = Self::new(project_id, root, StoreMode::ManagedLocal);
        store.published_repository_root = Some(code_repository.into());
        #[cfg(test)]
        {
            crate::persistence::dependency_authorization::identity::register_test_local_project(
                &store.root,
                &project_id.hyphenated().to_string(),
            );
        }
        store
    }

    pub fn layout(&self) -> PlanningLayout {
        PlanningLayout::for_store(self)
    }

    /// Root of this installation-local operational store.
    pub fn local_root(&self) -> PathBuf {
        self.root.clone()
    }

    /// Preserve a validated store-relative path in transaction plans.
    pub fn relative_path(&self, relative: &str) -> String {
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
    /// transaction that changes singleton records alongside independent ones.
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
