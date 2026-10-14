use serde::{Deserialize, Serialize};

use super::{is_git_oid, is_uuid, valid_text};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaFindingCategory {
    Functional,
    Visual,
    Accessibility,
    Usability,
    Performance,
    Crash,
    Regression,
    Security,
    Environment,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaFindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaFindingV1 {
    pub finding_uid: String,
    pub code_sha: String,
    pub scenario_uid: String,
    pub requirement_uids: Vec<String>,
    pub category: QaFindingCategory,
    pub severity: QaFindingSeverity,
    pub expected: String,
    pub actual: String,
    pub reproducible_steps: Vec<String>,
    pub screenshot_ids: Vec<String>,
    pub trace_ids: Vec<String>,
    pub smallest_fix: String,
    pub blocking: bool,
}

impl QaFindingV1 {
    pub fn validate(&self, manifest: &super::QaEvidenceManifestV1) -> Result<(), String> {
        if !is_uuid(&self.finding_uid)
            || self.code_sha != manifest.code_sha
            || !is_git_oid(&self.code_sha)
            || !manifest.scenario_ids.contains(&self.scenario_uid)
            || self.requirement_uids.is_empty()
            || self.requirement_uids.len() > 256
            || self.requirement_uids.iter().any(|id| {
                !manifest
                    .assertions
                    .iter()
                    .any(|assertion| assertion.requirement_ids.contains(id))
            })
            || !valid_text(&self.expected, 4096)
            || !valid_text(&self.actual, 4096)
            || self.reproducible_steps.is_empty()
            || self.reproducible_steps.len() > 128
            || self
                .reproducible_steps
                .iter()
                .any(|step| !valid_text(step, 4096))
            || self.screenshot_ids.iter().any(|id| {
                !manifest
                    .screenshots
                    .iter()
                    .any(|screenshot| &screenshot.id == id)
            })
            || self.trace_ids.iter().any(|id| {
                id.parse::<u32>()
                    .map_or(true, |index| index as usize >= manifest.actions.len())
            })
            || self.screenshot_ids.len() > 128
            || self.trace_ids.len() > 256
            || !valid_text(&self.smallest_fix, 4096)
        {
            return Err("runtime QA finding is invalid or lacks reproducible evidence".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaFindingDispositionKind {
    RepairInScope,
    AskUser,
    PromoteStandaloneTask,
    PromoteSubtask,
    AcceptNonblockingRisk,
    ContestWithEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaFindingDispositionStatus {
    Open,
    Assigned,
    Repairing,
    AwaitingAnswer,
    Resolved,
    Promoted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaFindingDispositionV1 {
    pub schema_version: u16,
    pub finding_id: String,
    pub qa_run_id: String,
    pub parent_task_id: String,
    pub code_sha: String,
    pub kind: QaFindingDispositionKind,
    pub status: QaFindingDispositionStatus,
    pub reason: String,
    pub owner_item_uid: Option<String>,
    pub impacted_requirements: Vec<String>,
    pub decision_id: Option<String>,
    pub created_revision: u64,
    pub expected_revision: u64,
}

impl QaFindingDispositionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.finding_id)
            || !is_uuid(&self.qa_run_id)
            || !is_uuid(&self.parent_task_id)
            || !is_git_oid(&self.code_sha)
            || !valid_text(&self.reason, 4096)
            || self.owner_item_uid.as_ref().is_some_and(|id| !is_uuid(id))
            || self.decision_id.as_ref().is_some_and(|id| !is_uuid(id))
            || self.impacted_requirements.len() > 256
            || self
                .impacted_requirements
                .iter()
                .any(|id| !valid_text(id, 128))
            || self.expected_revision != self.created_revision
        {
            return Err("runtime QA finding disposition is invalid".into());
        }
        Ok(())
    }
}
