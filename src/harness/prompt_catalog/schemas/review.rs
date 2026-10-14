use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerdictV1 {
    pub subject_hash: String,
    pub pass: bool,
    pub coverage: Vec<CoverageV1>,
    pub findings: Vec<ReviewFindingV1>,
    pub untested: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageV1 {
    pub requirement: String,
    pub status: CoverageStatus,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Pass,
    Fail,
    Untested,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFindingV1 {
    pub id: String,
    pub requirement: String,
    pub severity: Severity,
    pub kind: FindingKind,
    pub location: String,
    pub evidence: Vec<String>,
    pub repro: Option<String>,
    pub proposed_correction: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocker,
    High,
    Medium,
    Low,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Correctness,
    Security,
    Requirements,
    TestGap,
    Regression,
    Scope,
    Other,
}
