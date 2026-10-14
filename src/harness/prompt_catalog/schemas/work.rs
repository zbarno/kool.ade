use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResultV1 {
    #[serde(alias = "schemaVersion")]
    pub schema_version: u32,
    pub status: WorkStatus,
    pub blocker_disposition: BlockerDisposition,
    pub summary: String,
    pub acceptance_criteria: Vec<AcceptanceCriterionV1>,
    pub verification: Vec<String>,
    pub remaining: Vec<String>,
    pub human_choices: Vec<HumanChoiceV1>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Complete,
    Blocked,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerDisposition {
    None,
    MachineRepair,
    HumanAction,
    EnvironmentPrerequisite,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterionV1 {
    pub criterion: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanChoiceV1 {
    pub id: String,
    pub label: String,
    pub meaning: String,
    pub consequence: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecDeltaV1 {
    pub deltas: Vec<SpecChangeV1>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecChangeV1 {
    pub requirement_id: String,
    pub old: String,
    pub new: String,
    pub classification: DeltaClassification,
    pub evidence: Vec<String>,
    pub approval_effect: ApprovalEffect,
    pub followup_task_ids: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaClassification {
    CurrentTruth,
    FutureTarget,
    Documentation,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalEffect {
    None,
    RequiresUserApproval,
    DoesNotChangeApproval,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrProposalV1 {
    pub task_uid: String,
    pub base_ref: String,
    pub head_ref: String,
    pub commits: Vec<String>,
    pub files: Vec<String>,
    pub evidence: Vec<String>,
    pub description: String,
}
