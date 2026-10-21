use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeReviewDispositionStatusV1 {
    Open,
    AssignedRepair,
    PromotedFollowUp,
    RiskApproved,
    Resolved,
    Contested,
}

pub const MAX_CODE_REVIEW_ARBITRATION_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeReviewFindingDispositionV1 {
    pub finding_id: String,
    pub subject_hash: String,
    #[serde(default)]
    pub scope: Option<crate::harness::prompt_catalog::FindingScopeV1>,
    #[serde(default)]
    pub finding_fingerprint: Option<String>,
    #[serde(default = "default_occurrence_count")]
    pub occurrence_count: u32,
    #[serde(default)]
    pub arbitration_attempts: u32,
    pub status: CodeReviewDispositionStatusV1,
    pub decision_reason: String,
    pub owner_work_id: String,
    pub evidence_ref: String,
}

fn default_occurrence_count() -> u32 {
    1
}
