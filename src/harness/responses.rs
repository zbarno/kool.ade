//! Operation-specific wire response decoding and normalization.

use serde::de::DeserializeOwned;

use crate::core::workflow::{TaskOutline, TaskStory, TurnPurpose};
use crate::harness::TurnEnvelope;

mod wire;
pub use wire::{
    InvestigationResponse, PlanningTurnResponse, ReconciliationResponse, TaskGenerationResponse,
    TaskOutlineResponse, TaskStoryResponse,
};

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
        if purpose == TurnPurpose::ComparePlans {
            let value: serde_json::Value =
                serde_json::from_str(json).map_err(|error| error.to_string())?;
            if let Some(object) = value.as_object() {
                for field in ["task_stories", "taskStories", "task_outline", "taskOutline"] {
                    if object.contains_key(field) {
                        return Err(format!(
                            "Compare Plans forbids cross-purpose field `{field}`; return only the two plans and recommendation"
                        ));
                    }
                }
            }
        }
        let planning_json = normalize_planning_noops(json)?;
        let response = serde_json::from_str::<PlanningTurnResponse>(&planning_json)
            .map_err(|error| error.to_string())?;
        if purpose == TurnPurpose::ComparePlans {
            check_version("Compare plans", response.schema_version, &[2])?;
            return Ok(response.into());
        }
        // Schema 1 is accepted only at this decoder boundary so old
        // single-document responses cannot leak version logic into core.
        check_version("Planning", response.schema_version, &[1, 2])?;
        Ok(response.into())
    }
}

/// Some models include a known cross-purpose field with a null value even
/// when the operation-specific instructions forbid it. A null task-story
/// field carries no payload, so discard only that no-op spelling before the
/// strict planning schema decode. Non-null story data remains unknown here
/// and is still rejected at the boundary.
fn normalize_planning_noops(json: &str) -> Result<String, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    if let Some(object) = value.as_object_mut() {
        for field in ["task_stories", "taskStories"] {
            if object.get(field).is_some_and(serde_json::Value::is_null) {
                object.remove(field);
            }
        }
        // Some models mark unchanged product modules with a prose sentinel.
        // Product documents have no lifecycle status; discard only this exact
        // no-op spelling there. Feature status remains strictly typed.
        if let Some(updates) = object
            .get_mut("document_updates")
            .and_then(serde_json::Value::as_array_mut)
        {
            for update in updates {
                let Some(update) = update.as_object_mut() else {
                    continue;
                };
                let is_product = update
                    .get("document_id")
                    .or_else(|| update.get("documentId"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.starts_with("product:"));
                if is_product
                    && update.get("status").and_then(serde_json::Value::as_str)
                        == Some("unchanged-placeholder")
                {
                    update.remove("status");
                }
            }
        }
    }
    serde_json::to_string(&value).map_err(|error| error.to_string())
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
