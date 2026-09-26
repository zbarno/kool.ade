//! Incrementally saved task stories with an approved specification snapshot.
mod batch_write;
mod board;
mod generation;
mod identity;
mod metadata;
mod naming;
mod progress;
mod workflow;

#[cfg(test)]
mod naming_tests;

pub use batch_write::write_batch;
pub use board::{load_board, load_latest};
pub use generation::save_progress;
pub(crate) use identity::visible_content;
pub use metadata::TaskMetadata;
pub(crate) use metadata::legacy_dependencies;
pub(crate) use metadata::parse as parse_metadata;
pub use naming::{is_task_story_filename, slug};
pub use workflow::{load_workflow, safe_directory, save_workflow};

#[derive(Debug, Clone)]
pub struct TaskDocument {
    pub path: String,
    pub title: String,
    pub text: String,
    pub identity: Option<crate::domain::ArtifactIdentity>,
    pub metadata: Option<TaskMetadata>,
    pub metadata_error: Option<String>,
}
