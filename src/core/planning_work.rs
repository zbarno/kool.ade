//! Versioned durable work records for planning requests that may not yet have
//! produced a feature specification.
use serde::{Deserialize, Serialize};

pub const FILE: &str = crate::artifacts::layout::store::WORK;
pub const STORE_FILE: &str = crate::artifacts::planning_store::paths::WORK;
const SCHEMA_VERSION: u32 = 5;

mod code_review_disposition;
mod cycles;
mod history;
mod intent;
mod projection;
mod storage;
mod validation;
pub use code_review_disposition::{
    CodeReviewDispositionStatusV1, CodeReviewFindingDispositionV1,
    MAX_CODE_REVIEW_ARBITRATION_ATTEMPTS,
};
pub use cycles::{MAX_TASK_INTENT_CYCLE_GOAL_BYTES, TaskIntentCycleV1, start_follow_up_cycle};
pub use history::{
    TaskIntentEventEvidenceV1, TaskIntentEventKindV1, TaskIntentEventV1, TaskIntentGateKindV1,
    capture_authoritative_events,
};
pub(super) use intent::normalize_task_intents;
use validation::validate;
pub use validation::{routing_for_feature, save, save_expected};
mod answered;
mod reconcile;
pub use answered::reconcile_answered_child;
#[cfg(test)]
mod tests;

pub use projection::{cards, context, find_card};
pub use reconcile::{completed_turn_status, reconcile_inactive, reconcile_inactive_excluding};
pub(crate) use storage::linked_executable_task_changes;
pub use storage::{append_discovered, find, load, load_expected, record_changes};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    #[default]
    Feature,
    ProjectPlanning,
    Bug,
    NewProject,
    DocumentationRefresh,
    Question,
    TaskGeneration,
    PlanReviewDisposition,
    CodeReviewDisposition,
    QaFindingDisposition,
}

impl WorkKind {
    /// New Task request types that create a durable TaskIntent root.
    pub fn starts_task_intent(self) -> bool {
        matches!(
            self,
            Self::Feature
                | Self::Bug
                | Self::NewProject
                | Self::DocumentationRefresh
                | Self::Question
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Feature => "Feature",
            Self::ProjectPlanning => "Project planning",
            Self::Bug => "Bug",
            Self::NewProject => "New Project",
            Self::DocumentationRefresh => "Refresh Documentation",
            Self::Question => "Question",
            Self::TaskGeneration => "Task generation",
            Self::PlanReviewDisposition => "Plan review",
            Self::CodeReviewDisposition => "Code review",
            Self::QaFindingDisposition => "Runtime QA finding",
        }
    }

    pub fn harness_route_category(self) -> &'static str {
        use crate::persistence::harness_settings as routes;
        match self {
            Self::Feature => routes::FEATURE_PLANNING,
            Self::ProjectPlanning => routes::FEATURE_PLANNING,
            Self::Bug | Self::Question => routes::INVESTIGATION,
            Self::NewProject => routes::SPECIFICATION,
            Self::DocumentationRefresh => routes::DOCUMENTATION,
            Self::TaskGeneration => routes::TASK_GENERATION,
            Self::PlanReviewDisposition
            | Self::CodeReviewDisposition
            | Self::QaFindingDisposition => routes::MANAGER,
        }
    }

    pub fn planning_guidance(self) -> &'static str {
        match self {
            Self::Feature => {
                "Plan the requested new behavior and create a concise feature specification. Ground it in relevant existing documents and avoid boilerplate or repeating details in multiple sections."
            }
            Self::ProjectPlanning => {
                "Continue the shared project specification conversation. Preserve its global identity and context; create a TaskIntent only when the user explicitly starts a typed New Task."
            }
            Self::Bug => {
                "Inspect current behavior and repository evidence in bug-triage mode: first reproduce the reported failure or trace the relevant execution path, then identify and record the most likely root cause with exact repository evidence before planning a fix. Separate observed facts from hypotheses, and state what evidence would confirm or rule out each hypothesis. Inspect only the relevant repository slice and continue in bounded chunks; do not attempt a whole-repository survey. Once the cause is supported, write a concise corrective specification with the smallest fix, affected behavior, regression checks, and any risks. Do not plan a speculative fix while the cause is unknown. Do not ask about already established intended behavior unless expected behavior is genuinely unclear; ask only for missing reproduction details or evidence the user uniquely has. If evidence cannot be obtained safely, record the precise blocker and next diagnostic step rather than guessing. Avoid duplicating the report or repeating a requirement across sections."
            }
            Self::NewProject => {
                "Establish the product problem, users, outcome, scope, constraints, major behaviors, architecture, risks, and unknowns progressively. Record only agreed or evidenced details; do not fill unknown sections with generic boilerplate or repeat the same fact."
            }
            Self::DocumentationRefresh => {
                "Analyze the existing repository to build or refresh its project documentation. Start from existing repo-level documentation and inspect source code to verify and fill gaps. If Kool.ad/e project artifacts are missing or incomplete, create useful evidence-based product documents instead of assuming an existing specification is complete. Record contradictions, missing decisions, and unresolved questions as Needs Attention tasks. Record suspected bugs, vulnerabilities, and other quality risks as Todo triage tasks with evidence and uncertainty clearly stated. Do not fix findings or propose implementation as if confirmed. Preserve established facts, avoid boilerplate, and continue in bounded coherent slices until the codebase has been adequately documented."
            }
            Self::Question => {
                "Investigate and answer directly with repository evidence. Do not create a specification unless the answer uncovers a separate user decision."
            }
            Self::TaskGeneration => {
                "Generate implementation task stories from the current explicitly approved feature specification."
            }
            Self::PlanReviewDisposition => {
                "Resolve one Manager disposition for a persisted independent plan-review finding. Preserve the user's goal, cite the reviewed evidence, and require fresh user confirmation after any plan change."
            }
            Self::CodeReviewDisposition => {
                "Resolve one Manager disposition for a persisted independent code-review finding. Preserve the approved specification, make the smallest scoped repair, and require fresh verification and independent review after code changes."
            }
            Self::QaFindingDisposition => {
                "Review one persisted runtime QA finding and propose a repair within the approved requirements. Bind the proposal to the exact failed QA run and code revision; do not broaden or change approved product scope."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    #[default]
    Todo,
    InProgress,
    InReview,
    NeedsAttention,
    Done,
}

impl WorkStatus {
    /// Board layout projection; status remains the persisted workflow state.
    pub fn board_column(self) -> usize {
        match self {
            Self::Todo => 0,
            Self::InProgress => 1,
            Self::InReview => 2,
            Self::NeedsAttention => 3,
            Self::Done => 4,
        }
    }

    fn from_legacy_column(column: usize) -> anyhow::Result<Self> {
        match column {
            0 => Ok(Self::Todo),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::InReview),
            3 => Ok(Self::NeedsAttention),
            4 => Ok(Self::Done),
            _ => anyhow::bail!("Unsupported legacy planning work column {column}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub uid: String,
    pub key: String,
    pub kind: WorkKind,
    pub title: String,
    /// Immutable first user-authored goal retained as the request is refined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_goal: Option<String>,
    pub request: String,
    pub status: WorkStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uid: Option<String>,
    /// Canonical TaskIntent root that owns this planning work. For a root
    /// request this is its own stable UID; descendants keep the same UID even
    /// when their nesting and dependency relationships differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_intent_uid: Option<String>,
    /// Append-only shared history for the TaskIntent root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_intent_history: Vec<TaskIntentEventV1>,
    /// Explicit work cycles preserve earlier approved or released outcomes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_intent_cycles: Vec<TaskIntentCycleV1>,
    /// Cycle for this planning work; descendants inherit it from their parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_intent_cycle_uid: Option<String>,
    /// Shared archive marker; history remains on the root Work record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_intent_archived_at_ms: Option<u64>,
    #[serde(
        default,
        alias = "source_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_branch: Option<String>,
    #[serde(
        default,
        alias = "destination_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub destination_branch: Option<String>,
    /// Explicit task-owned routes; missing categories remain application defaults.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub routing_overrides:
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    /// UID of the related task whose route overrides were copied here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_inherited_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up_task: Option<FollowUpTaskOffer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_review_disposition: Option<CodeReviewFindingDispositionV1>,
    pub detail: String,
    /// Revision captured when this record was loaded; not part of legacy
    /// aggregate serialization.
    #[serde(skip)]
    pub record_revision: u64,
    /// Serialized content captured at load time so full board snapshots only
    /// write records this caller actually changed.
    #[serde(skip)]
    #[doc(hidden)]
    pub record_baseline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowUpTaskOffer {
    pub title: String,
    pub description: String,
}

/// Match a planning record to the feature it owns. A durable UID is
/// authoritative; the display ID is only a migration fallback for legacy
/// records that predate feature UIDs.
pub(crate) fn belongs_to_feature(work: &Work, feature_id: &str, feature_uid: Option<&str>) -> bool {
    match (work.feature_uid.as_deref(), feature_uid) {
        (Some(work_uid), Some(feature_uid)) => work_uid == feature_uid,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => work.feature_id.as_deref() == Some(feature_id),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkFile {
    schema_version: u32,
    items: Vec<Work>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkRecord {
    schema_version: u32,
    uid: String,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    data: Work,
}
