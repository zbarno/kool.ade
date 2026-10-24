use serde::{Deserialize, Serialize};

use super::{TurnItem, TurnItemUpdate};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_remediation: Option<crate::core::lifecycle::PlanRemediationV1>,
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
