use std::path::PathBuf;

/// Actions exposed by the planning repository settings page.
pub enum PlanningRepositoryCommand {
    SyncNow,
    ResolveConflict {
        operation_id: uuid::Uuid,
        path: String,
        choice: crate::core::planning_sync::PlanningConflictChoice,
        manual_value: Option<String>,
    },
    CancelPendingOperation {
        operation_id: uuid::Uuid,
    },
    ExportPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningCodeRepositoryView {
    pub id: String,
    pub remote: String,
    pub checkout: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningRepositoryViewModel {
    pub workspace_id: String,
    pub local_path: PathBuf,
    pub shared: bool,
    pub canonical_ref: String,
    pub remote_url: Option<String>,
    pub repositories: Vec<PlanningCodeRepositoryView>,
    pub sync: crate::core::planning_sync::PlanningSyncStatus,
    pub sync_running: bool,
}
