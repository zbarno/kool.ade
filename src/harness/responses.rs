//! Operation-specific wire responses. Decode these strictly at the boundary,
//! then normalize into the planning pipeline's internal model.

use serde::{Deserialize, de::DeserializeOwned};

use crate::core::workflow::{InterviewBrief, TaskOutline, TaskStory, TurnPurpose};
use crate::domain::{PlanAlternative, PlanRecommendation};

use super::{DocumentUpdate, RequestedAction, TurnEnvelope, TurnItem, TurnItemUpdate};

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
            interview: None,
            task_stories: None,
            task_outline: None,
            plans: None,
            recommendation: None,
        }
    }
}

/// A model-generated blocker explanation; the alias gives the wire operation
/// a stable name while reusing its persisted, UI-facing domain model.
pub type DecisionBriefResponse = crate::core::attention::Brief;

pub fn decode_turn(text: &str, purpose: TurnPurpose) -> Result<TurnEnvelope, String> {
    let json = response_object(text)?;
    decode_turn_object(&json, purpose)
}

pub fn decode_turn_object(json: &str, purpose: TurnPurpose) -> Result<TurnEnvelope, String> {
    if purpose == TurnPurpose::GenerateTasks {
        let response = serde_json::from_str::<TaskGenerationResponse>(json)
            .map_err(|error| error.to_string())?;
        check_version("Task generation", response.schema_version, &[1])?;
        Ok(response.into())
    } else {
        let response = serde_json::from_str::<PlanningTurnResponse>(json)
            .map_err(|error| error.to_string())?;
        // Schema 1 is accepted only at this decoder boundary so old
        // single-document responses cannot leak version logic into core.
        check_version("Planning", response.schema_version, &[1, 2])?;
        Ok(response.into())
    }
}

pub fn decode_investigation(text: &str) -> Result<InvestigationResponse, String> {
    let response = decode::<InvestigationResponse>(text)?;
    check_version("Investigation", response.schema_version, &[2])?;
    Ok(response)
}

pub fn decode_reconciliation(text: &str) -> Result<ReconciliationResponse, String> {
    let response = decode::<ReconciliationResponse>(text)?;
    check_version("Reconciliation", response.schema_version, &[2])?;
    Ok(response)
}

fn check_version(operation: &str, version: Option<u32>, supported: &[u32]) -> Result<(), String> {
    if let Some(version) = version
        && !supported.contains(&version)
    {
        return Err(format!(
            "{operation} schema_version {version} is unsupported; expected {}",
            supported
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" or ")
        ));
    }
    Ok(())
}

pub fn decode<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let json = response_object(text)?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

fn response_object(text: &str) -> Result<String, String> {
    crate::harness::pi_extract::extract_json_object(text)
        .ok_or_else(|| "response has no complete JSON object".to_owned())
}

pub fn normalize_task_outline(response: TaskOutlineResponse) -> Result<Vec<TaskOutline>, String> {
    if response.schema_version.is_some_and(|version| version != 1) {
        return Err("Task outline schema_version must be 1".into());
    }
    if response
        .task_stories
        .as_ref()
        .is_some_and(|stories| !stories.is_empty())
    {
        return Err("Task outline responses may not include detailed task stories".into());
    }
    Ok(response.task_outline.unwrap_or_default())
}

pub fn normalize_task_story(response: TaskStoryResponse) -> Result<Vec<TaskStory>, String> {
    if response.schema_version.is_some_and(|version| version != 1) {
        return Err("Task story schema_version must be 1".into());
    }
    Ok(response.task_stories.unwrap_or_default())
}

pub fn decode_decision_brief(text: &str) -> Result<DecisionBriefResponse, String> {
    decode(text)
}

#[cfg(test)]
#[path = "responses/tests.rs"]
mod tests;
