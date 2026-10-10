//! Incrementally saved task stories with an approved specification snapshot.
mod batch_write;
mod board;
mod generation;
mod identity;
mod metadata;
mod naming;
mod progress;
mod record_files;
mod task_state;
mod workflow;

#[cfg(test)]
mod naming_tests;

pub use batch_write::{write_batch, write_batch_expected, write_batch_expected_with_revision};
pub use board::{load_board, load_latest};
pub use generation::{save_progress, save_progress_expected};
pub(crate) use identity::visible_content;
pub use metadata::TaskMetadata;
pub(crate) use metadata::legacy_dependencies;
pub(crate) use metadata::parse as parse_metadata;
pub use naming::{is_task_story_filename, slug};
pub use task_state::TaskState;
pub(crate) use workflow::load_workflow_unlocked;
pub use workflow::{load_workflow, safe_directory, save_workflow};

pub(crate) fn workflow_record_changes<
    R: crate::artifacts::planning_store::PlanningRoot + ?Sized,
>(
    repo: &R,
    workflow: &crate::core::workflow::Workflow,
) -> anyhow::Result<crate::artifacts::planning_store::RecordChangePlan> {
    record_files::changes(repo, workflow)
}

pub(crate) fn update_task_execution_status(
    store: &crate::artifacts::planning_store::PlanningStore,
    metadata: &TaskMetadata,
    expected_revision: u64,
    status: crate::core::planning_work::WorkStatus,
    execution_status: &str,
) -> anyhow::Result<TaskState> {
    task_state::update_execution_status(
        store,
        metadata,
        expected_revision,
        status,
        execution_status,
    )
}

pub(crate) fn backfill_missing_task_records(
    store: &crate::artifacts::planning_store::PlanningStore,
    workflow: &crate::core::workflow::Workflow,
) -> anyhow::Result<Vec<String>> {
    task_state::backfill_missing(store, workflow)
}

#[derive(Debug, Clone)]
pub struct TaskDocument {
    pub path: String,
    pub title: String,
    pub text: String,
    pub identity: Option<crate::domain::ArtifactIdentity>,
    pub metadata: Option<TaskMetadata>,
    pub task_state: Option<TaskState>,
    pub metadata_error: Option<String>,
}
