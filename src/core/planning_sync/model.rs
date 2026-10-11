use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningSyncMode {
    Local,
    Shared,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningSyncState {
    Local,
    Synced,
    #[default]
    Pending,
    Offline,
    AuthRequired,
    Conflicted,
    ReadOnly,
    IncompatibleSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningSyncConflict {
    pub operation_id: uuid::Uuid,
    pub path: String,
    pub record_id: String,
    pub expected_revision: Option<u64>,
    pub base_commit: String,
    pub remote_commit: String,
    pub fields: Vec<String>,
    pub local_value: Option<String>,
    pub remote_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningSyncStatus {
    pub mode: PlanningSyncMode,
    pub state: PlanningSyncState,
    pub canonical_ref: Option<String>,
    pub confirmed_commit: Option<String>,
    pub confirmed_at_ms: Option<u64>,
    pub pending_operations: usize,
    pub conflicts: Vec<PlanningSyncConflict>,
    pub detail: Option<String>,
}

impl PlanningSyncStatus {
    pub fn local() -> Self {
        Self {
            mode: PlanningSyncMode::Local,
            state: PlanningSyncState::Local,
            canonical_ref: None,
            confirmed_commit: None,
            confirmed_at_ms: None,
            pending_operations: 0,
            conflicts: Vec::new(),
            detail: None,
        }
    }
}
