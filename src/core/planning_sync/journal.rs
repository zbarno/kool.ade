use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{
    PlanningSyncConflict, PlanningSyncMode, PlanningSyncPendingOperation, PlanningSyncState,
    PlanningSyncStatus,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OperationJournal {
    pub schema_version: u32,
    pub workspace_id: uuid::Uuid,
    pub canonical_ref: String,
    #[serde(default)]
    pub remote_identity: Option<String>,
    #[serde(default)]
    pub confirmed_commit: Option<String>,
    #[serde(default)]
    pub confirmed_local_commit: Option<String>,
    #[serde(default)]
    pub confirmed_at_ms: Option<u64>,
    #[serde(default)]
    pub operations: Vec<SyncOperation>,
    #[serde(default)]
    pub conflicts: Vec<PlanningSyncConflict>,
    #[serde(default)]
    pub resolutions: Vec<PlanningSyncResolution>,
    #[serde(default)]
    pub state: PlanningSyncState,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlanningSyncResolution {
    pub resolution_id: uuid::Uuid,
    pub source_operation_id: uuid::Uuid,
    pub path: String,
    pub choice: super::model::PlanningConflictChoice,
    pub remote_commit: String,
    pub new_operation_id: Option<uuid::Uuid>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncOperation {
    pub operation_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub base_commit: String,
    pub canonical_ref: String,
    pub message: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub remote_base_commit: Option<String>,
    #[serde(default)]
    pub local_commit: Option<String>,
    pub mutations: Vec<RecordMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecordMutation {
    pub path: String,
    pub record_id: String,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub desired: Option<String>,
}

impl OperationJournal {
    pub fn new(workspace_id: uuid::Uuid, canonical_ref: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            workspace_id,
            canonical_ref: canonical_ref.to_owned(),
            remote_identity: None,
            confirmed_commit: None,
            confirmed_local_commit: None,
            confirmed_at_ms: None,
            operations: Vec::new(),
            conflicts: Vec::new(),
            resolutions: Vec::new(),
            state: PlanningSyncState::Pending,
            detail: None,
        }
    }

    pub fn load_or_new(
        state_root: &Path,
        workspace_id: uuid::Uuid,
        canonical_ref: &str,
    ) -> anyhow::Result<Self> {
        let path = path(state_root, workspace_id);
        let journal = if path.is_file() {
            serde_json::from_slice::<Self>(&std::fs::read(&path)?)?
        } else {
            return Ok(Self::new(workspace_id, canonical_ref));
        };
        anyhow::ensure!(
            journal.schema_version == SCHEMA_VERSION,
            "unsupported local planning sync journal schema"
        );
        anyhow::ensure!(
            journal.workspace_id == workspace_id,
            "local planning sync journal workspace identity mismatch"
        );
        anyhow::ensure!(
            journal.canonical_ref == canonical_ref,
            "local planning sync journal canonical ref changed"
        );
        anyhow::ensure!(
            journal
                .operations
                .iter()
                .all(|operation| operation.workspace_id == workspace_id
                    && operation.canonical_ref == canonical_ref
                    && operation
                        .mutations
                        .iter()
                        .all(|mutation| valid_sync_path(&mutation.path))),
            "local planning sync journal contains a foreign or invalid operation"
        );
        Ok(journal)
    }

    pub fn write(&self, state_root: &Path) -> anyhow::Result<()> {
        let path = path(state_root, self.workspace_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn status(&self) -> PlanningSyncStatus {
        let pending = self
            .operations
            .iter()
            .map(|operation| {
                let mut target_ids = operation
                    .mutations
                    .iter()
                    .map(|mutation| mutation.record_id.clone())
                    .collect::<Vec<_>>();
                target_ids.sort();
                target_ids.dedup();
                PlanningSyncPendingOperation {
                    operation_id: operation.operation_id,
                    created_at_ms: operation.created_at_ms,
                    summary: operation.message.clone(),
                    target_ids,
                    has_local_commit: operation.local_commit.is_some(),
                }
            })
            .collect::<Vec<_>>();
        PlanningSyncStatus {
            mode: PlanningSyncMode::Shared,
            state: self.state.clone(),
            canonical_ref: Some(self.canonical_ref.clone()),
            confirmed_commit: self.confirmed_commit.clone(),
            confirmed_at_ms: self.confirmed_at_ms,
            pending_operations: pending.len(),
            pending,
            conflicts: self.conflicts.clone(),
            detail: self.detail.clone(),
        }
    }
}

fn valid_sync_path(path: &str) -> bool {
    let relative = Path::new(path);
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !relative.is_absolute()
        && relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        && (path.starts_with("state/") || path.starts_with("planning/"))
        && (!super::merge::is_record_collection_path(path)
            || super::merge::record_json_uid(path).is_some()
            || super::merge::record_markdown_uid(path).is_some())
        && !matches!(
            path,
            "state/work.json"
                | "state/workflow.json"
                | "planning/open-items.md"
                | "planning/resolved-items.json"
        )
}

pub(super) fn path(state_root: &Path, workspace_id: uuid::Uuid) -> PathBuf {
    state_root
        .join("projects")
        .join(workspace_id.hyphenated().to_string())
        .join("sync-operations.json")
}
