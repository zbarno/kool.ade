use serde::{Deserialize, Serialize};

use super::types::{GateReceipt, TaskPhase};

mod events;
mod qa;
mod transitions;
pub use events::TaskEvent;
pub use qa::{QaExecutionStateV1, QaExecutionStatusV1};

const MAX_REVIEW_ATTEMPTS: usize = 8;
const MAX_REPAIR_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub schema_version: u16,
    pub task_uid: String,
    pub feature_uid: String,
    /// UID of the persisted planning item this lifecycle identity tracks.
    #[serde(default)]
    pub planning_item_uid: Option<String>,
    /// Hash of the task content, domain-bound to `approved_plan_sha256`.
    #[serde(default)]
    pub task_spec_sha256: Option<String>,
    /// Subtasks belong to exactly one standalone task and do not create their
    /// own pull request. `None` identifies a standalone task.
    #[serde(default)]
    pub parent_task_uid: Option<String>,
    /// Stable direct children owned by this standalone task.
    #[serde(default)]
    pub subtask_uids: Vec<String>,
    /// Approved implementation dependencies within the same task batch.
    #[serde(default)]
    pub dependency_task_uids: Vec<String>,
    /// Stable requirements assigned by the approved task decomposition.
    /// Legacy task records load with no inferred requirement links.
    #[serde(default)]
    pub approved_requirement_ids: Vec<String>,
    pub phase: TaskPhase,
    pub revision: u64,
    pub approved_plan_sha256: String,
    /// Diff hash reviewed by CodeReviewer. Kept separate from the Git commit OID.
    pub implementation_sha256: Option<String>,
    /// Exact verified task-clone commit included in the CodeReviewer subject.
    #[serde(default)]
    pub reviewed_code_head_sha: Option<String>,
    /// Hash of the complete immutable CodeReviewer subject, including the head.
    #[serde(default)]
    pub reviewed_code_subject_sha256: Option<String>,
    pub code_review: Option<GateReceipt>,
    /// Append-only outcomes for independent reviews across repair cycles.
    #[serde(default)]
    pub review_history: Vec<GateReceipt>,
    #[serde(default)]
    pub repair_attempts: u8,
    pub runtime_qa: Option<GateReceipt>,
    #[serde(default)]
    pub qa_execution: Option<QaExecutionStateV1>,
    pub spec_reconciliation: Option<GateReceipt>,
    #[serde(default)]
    pub spec_reconciliation_blocker: Option<String>,
    #[serde(default)]
    pub spec_reconciliation_proposal_sha256: Option<String>,
    #[serde(default)]
    pub spec_reconciliation_spec_sha256: Option<String>,
    /// Canonical validated SpecDelta proposal awaiting explicit user resolution.
    #[serde(default)]
    pub spec_reconciliation_proposal_json: Option<String>,
    pub pr_work_item_uid: Option<String>,
}

mod validation;
