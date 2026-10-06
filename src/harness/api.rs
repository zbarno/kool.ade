//! Narrow harness boundary (SPECIFICATION.md §15).
//!
//! ```text
//! AiHarness
//!   └── PiHarness      (MVP implementation; see `pi_harness`)
//! ```
//!
//! The rest of the application only sees [`AiHarness`]: it hands over a
//! [`PlanningRequest`] and receives either a [`HarnessOutcome`] or a typed
//! [`crate::AppError`]. Nothing Pi-specific leaks upward. Future harnesses
//! (Codex CLI, Copilot CLI, Claude Code…) plug in by implementing the trait.

use serde::{Deserialize, Serialize};

use crate::error::AppError;

mod activity;
mod execution;
pub use activity::{ActivityTelemetry, LivePost, LiveProgress, ModelCallUsage};
pub use execution::{ExecutionMode, PlanningRequest, ToolAccess};

/// Model-selected logical sources for a bounded planning turn. References are
/// suggestions only; the application resolves each against its current catalog.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetrievalPlan {
    pub documents: Vec<String>,
    pub open_items: Vec<String>,
    pub repository_areas: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentUpdate {
    #[serde(alias = "document_id")]
    pub document_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<crate::domain::ChangeStatus>,
}

/// A follow-up task discovered during a documentation refresh. Findings stay
/// advisory until a person triages them; this record never requests a code fix.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningTaskDraft {
    pub title: String,
    pub description: String,
    pub kind: crate::core::planning_work::WorkKind,
    pub status: crate::core::planning_work::WorkStatus,
}

/// Normalized internal planning projection. Raw operation responses are
/// decoded through `harness::responses` before reaching this model.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEnvelope {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    /// Short phrase describing what changed (feeds the git commit subject).
    #[serde(alias = "change_summary")]
    pub change_summary: Option<String>,
    /// Full replacements of logical product or feature documents.
    #[serde(alias = "document_updates")]
    pub document_updates: Option<Vec<DocumentUpdate>>,
    #[serde(
        default,
        alias = "planning_tasks",
        skip_serializing_if = "Option::is_none"
    )]
    pub planning_tasks: Option<Vec<PlanningTaskDraft>>,
    /// Complete replacement specification markdown (null when unchanged).
    #[serde(alias = "updated_specification")]
    pub updated_specification: Option<String>,
    #[serde(alias = "open_items_added")]
    pub open_items_added: Option<Vec<TurnItem>>,
    #[serde(alias = "open_items_updated")]
    pub open_items_updated: Option<Vec<TurnItemUpdate>>,
    #[serde(alias = "open_items_resolved")]
    pub open_items_resolved: Option<Vec<String>>,
    /// The item the agent wants to ask NOW (must satisfy routing rules).
    #[serde(alias = "next_question_id")]
    pub next_question_id: Option<String>,
    /// A typed interpretation of an explicit user request. Rust rechecks the
    /// action against the current workflow before dispatching it.
    #[serde(default, alias = "requested_action")]
    pub requested_action: Option<RequestedAction>,
    #[serde(default, alias = "follow_up_task")]
    pub follow_up_task: Option<PlanningTaskOffer>,
    pub interview: Option<crate::core::workflow::InterviewBrief>,
    #[serde(alias = "task_stories", skip_serializing_if = "Option::is_none")]
    pub task_stories: Option<Vec<crate::core::workflow::TaskStory>>,
    #[serde(alias = "task_outline", skip_serializing_if = "Option::is_none")]
    pub task_outline: Option<Vec<crate::core::workflow::TaskOutline>>,
    /// Koolade-authored alternatives and advisory lean for Compare Plans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plans: Option<Vec<crate::domain::PlanAlternative>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<crate::domain::PlanRecommendation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationAction {
    ApproveChange,
    GenerateTasks,
    StartImplementation,
    PauseImplementation,
    ResumeImplementation,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedAction {
    pub action: ApplicationAction,
    /// Stable Koolade identity for a specifically named change or task.
    #[serde(default, alias = "target_uid")]
    pub target_uid: Option<String>,
}

/// Optional user-consented follow-up offered after a focused Question task.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningTaskOffer {
    pub title: String,
    pub description: String,
}

impl TurnEnvelope {
    /// Lenient accessors: the contract asks for `[]`, but an agent that omits
    /// an empty list should not tank the whole turn.
    pub fn added(&self) -> &[TurnItem] {
        self.open_items_added.as_deref().unwrap_or_default()
    }
    pub fn updated(&self) -> &[TurnItemUpdate] {
        self.open_items_updated.as_deref().unwrap_or_default()
    }
    pub fn resolved(&self) -> &[String] {
        self.open_items_resolved.as_deref().unwrap_or_default()
    }
    pub fn assistant(&self) -> &str {
        self.assistant_message.as_deref().unwrap_or("")
    }
    pub fn updated_spec(&self) -> Option<&str> {
        self.updated_specification.as_deref()
    }
}

/// A brand-new open item proposed by the agent.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnItem {
    #[serde(default)]
    pub id: Option<String>,
    pub priority: Option<String>,
    pub authority: Option<String>,
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "feature_id")]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    #[serde(alias = "decision_brief")]
    pub decision_brief: Option<crate::domain::DecisionBrief>,
    #[serde(default, alias = "blocked_by")]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    #[serde(alias = "resolution_note")]
    pub resolution_note: Option<String>,
}

/// Partial update of an existing item (identified by `id`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnItemUpdate {
    pub id: Option<String>,
    pub priority: Option<String>,
    pub authority: Option<String>,
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "feature_id")]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    #[serde(alias = "decision_brief")]
    pub decision_brief: Option<crate::domain::DecisionBrief>,
    #[serde(default, alias = "blocked_by")]
    pub blocked_by: Option<Vec<String>>,
}

/// Successful harness termination.
#[derive(Debug, Clone)]
pub struct HarnessOutcome {
    /// Authoritative final assistant text (prose + JSON block).
    pub final_text: String,
    /// Decoded envelope when the final text carried a valid-looking JSON
    /// block. `None` ⇒ no block found or unparseable (fatal downstream).
    pub envelope: Option<TurnEnvelope>,
    /// Last few stderr lines, kept for diagnostics only.
    pub stderr_tail: String,
}

/// The boundary every AI backend implements.
pub trait AiHarness: Send + Sync {
    /// Human-readable backend label for the UI (e.g. "pi 0.84.4").
    fn label(&self) -> String;

    /// Availability probe (executables present, sane version).
    fn check_available(&self) -> Result<String, AppError>;

    /// Run one planning turn to completion (spawns/manages the process).
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError>;

    /// Select from Koolade's bounded source catalog before a main planning turn.
    /// Backends without a dedicated retrieval pass use Koolade's safe core context.
    fn plan_retrieval(
        &self,
        _request: &PlanningRequest,
    ) -> Result<Option<RetrievalPlan>, AppError> {
        Ok(None)
    }
}
