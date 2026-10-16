use serde::{Deserialize, Serialize};

use super::{FeatureEvent, TaskEvent, task_transition::PullRequestPreparationStatus};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleActor {
    User,
    Manager,
    IndependentReviewer,
    Worker,
    ApplicationPolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Snapshot<T> {
    pub(super) uid: String,
    pub(super) schema_version: u16,
    pub(super) created_at_ms: u64,
    pub(super) updated_at_ms: u64,
    pub(super) revision: u64,
    pub(super) record: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleEventRecord {
    pub uid: String,
    pub schema_version: u16,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub feature_uid: String,
    pub task_uid: Option<String>,
    pub expected_revision: u64,
    pub actor: LifecycleActor,
    pub event: LifecycleEventKind,
    /// Revision of the feature projection after this event when it changes
    /// the feature record. Older history records omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_revision_after: Option<u64>,
    /// Revision of the task projection after this event when it changes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_revision_after: Option<u64>,
    /// Initial feature state stored on new creation events for replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_feature: Option<super::FeatureGoal>,
    /// Initial task state stored on new task creation events for replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_task: Option<super::TaskRecord>,
    /// Parent task projection after a subtask identity is attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_after: Option<super::TaskRecord>,
    /// Complete PR-preparation projection after creation or state change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_preparation_after: Option<super::PullRequestPreparation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "scope", content = "event", rename_all = "snake_case")]
pub enum LifecycleEventKind {
    Feature(FeatureEvent),
    Task(TaskEvent),
    FeatureCreated,
    TaskCreated,
    SubtaskCreated {
        parent_task_uid: String,
    },
    PullRequestState {
        work_item_uid: String,
        status: PullRequestPreparationStatus,
    },
}
