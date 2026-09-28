//! Strict operation-specific wire response structs.
//! Operation-specific wire responses. Decode these strictly at the boundary,
//! then normalize into the planning pipeline's internal model.

use serde::Deserialize;

use crate::core::workflow::{InterviewBrief, TaskOutline, TaskStory};
use crate::domain::{PlanAlternative, PlanRecommendation};
use crate::harness::{
    DocumentUpdate, PlanningTaskOffer, RequestedAction, TurnEnvelope, TurnItem, TurnItemUpdate,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningTurnResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "change_summary")]
    pub change_summary: Option<String>,
    #[serde(alias = "document_updates")]
    pub document_updates: Option<Vec<DocumentUpdate>>,
    #[serde(alias = "updated_specification")]
    pub updated_specification: Option<String>,
    #[serde(alias = "open_items_added")]
    pub open_items_added: Option<Vec<TurnItem>>,
    #[serde(alias = "open_items_updated")]
    pub open_items_updated: Option<Vec<TurnItemUpdate>>,
    #[serde(alias = "open_items_resolved")]
    pub open_items_resolved: Option<Vec<String>>,
    #[serde(alias = "next_question_id")]
    pub next_question_id: Option<String>,
    #[serde(alias = "requested_action")]
    pub requested_action: Option<RequestedAction>,
    #[serde(alias = "follow_up_task")]
    pub follow_up_task: Option<PlanningTaskOffer>,
    pub interview: Option<InterviewBrief>,
    #[serde(alias = "plan_alternatives")]
    pub plans: Option<Vec<PlanAlternative>>,
    pub recommendation: Option<PlanRecommendation>,
}

impl From<PlanningTurnResponse> for TurnEnvelope {
    fn from(response: PlanningTurnResponse) -> Self {
        Self {
            // Raw legacy/current versions are resolved here; core sees only
            // the normalized current planning envelope.
            schema_version: Some(2),
            assistant_message: response.assistant_message,
            change_summary: response.change_summary,
            document_updates: response.document_updates,
            updated_specification: response.updated_specification,
            open_items_added: response.open_items_added,
            open_items_updated: response.open_items_updated,
            open_items_resolved: response.open_items_resolved,
            next_question_id: response.next_question_id,
            requested_action: response.requested_action,
            follow_up_task: response.follow_up_task,
            interview: response.interview,
            task_stories: None,
            task_outline: None,
            plans: response.plans,
            recommendation: response.recommendation,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskOutlineResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "task_stories")]
    pub task_stories: Option<Vec<TaskStory>>,
    #[serde(alias = "task_outline")]
    pub task_outline: Option<Vec<TaskOutline>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskStoryResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "task_stories")]
    pub task_stories: Option<Vec<TaskStory>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskGenerationResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "task_stories")]
    pub task_stories: Option<Vec<TaskStory>>,
}

impl From<TaskGenerationResponse> for TurnEnvelope {
    fn from(response: TaskGenerationResponse) -> Self {
        Self {
            schema_version: Some(2),
            assistant_message: response.assistant_message,
            task_stories: response.task_stories,
            change_summary: None,
            document_updates: None,
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            requested_action: None,
            follow_up_task: None,
            interview: None,
            task_outline: None,
            plans: None,
            recommendation: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestigationResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "change_summary")]
    pub change_summary: Option<String>,
    #[serde(alias = "document_updates")]
    pub document_updates: Option<Vec<DocumentUpdate>>,
    #[serde(alias = "open_items_added")]
    pub open_items_added: Option<Vec<TurnItem>>,
    #[serde(alias = "open_items_updated")]
    pub open_items_updated: Option<Vec<TurnItemUpdate>>,
    #[serde(alias = "open_items_resolved")]
    pub open_items_resolved: Option<Vec<String>>,
    #[serde(alias = "next_question_id")]
    pub next_question_id: Option<String>,
}

impl From<InvestigationResponse> for TurnEnvelope {
    fn from(response: InvestigationResponse) -> Self {
        Self {
            schema_version: Some(2),
            assistant_message: response.assistant_message,
            change_summary: response.change_summary,
            document_updates: response.document_updates,
            open_items_added: response.open_items_added,
            open_items_updated: response.open_items_updated,
            open_items_resolved: response.open_items_resolved,
            next_question_id: response.next_question_id,
            updated_specification: None,
            requested_action: None,
            follow_up_task: None,
            interview: None,
            task_stories: None,
            task_outline: None,
            plans: None,
            recommendation: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconciliationResponse {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    #[serde(alias = "change_summary")]
    pub change_summary: Option<String>,
    #[serde(alias = "document_updates")]
    pub document_updates: Option<Vec<DocumentUpdate>>,
    #[serde(alias = "open_items_added")]
    pub open_items_added: Option<Vec<TurnItem>>,
    #[serde(alias = "open_items_updated")]
    pub open_items_updated: Option<Vec<TurnItemUpdate>>,
    #[serde(alias = "open_items_resolved")]
    pub open_items_resolved: Option<Vec<String>>,
    #[serde(alias = "next_question_id")]
    pub next_question_id: Option<String>,
}

impl From<ReconciliationResponse> for TurnEnvelope {
    fn from(response: ReconciliationResponse) -> Self {
        Self {
            schema_version: Some(2),
            assistant_message: response.assistant_message,
            change_summary: response.change_summary,
            document_updates: response.document_updates,
            open_items_added: response.open_items_added,
            open_items_updated: response.open_items_updated,
            open_items_resolved: response.open_items_resolved,
            next_question_id: response.next_question_id,
            updated_specification: None,
            requested_action: None,
            follow_up_task: None,
            interview: None,
            task_stories: None,
            task_outline: None,
            plans: None,
            recommendation: None,
        }
    }
}
