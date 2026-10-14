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
pub(super) struct Snapshot<T> {
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
