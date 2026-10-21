use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerdictV1 {
    pub subject_hash: String,
    #[serde(default)]
    pub examined_files: Vec<String>,
    #[serde(default)]
    pub tested_evidence_ids: Vec<String>,
    #[serde(default)]
    pub unexamined_areas: Vec<String>,
    pub pass: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ReviewOutcomeV1>,
    pub coverage: Vec<CoverageV1>,
    pub findings: Vec<ReviewFindingV1>,
    pub untested: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewOutcomeV1 {
    Pass,
    BlockingFindings,
    ReviewUnable,
}
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageV1 {
    pub requirement: String,
    pub status: CoverageStatus,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Pass,
    Fail,
    Untested,
}
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFindingV1 {
    pub id: String,
    pub requirement: String,
    pub severity: Severity,
    #[serde(rename = "category", alias = "kind")]
    pub kind: FindingKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<FindingScopeV1>,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_detail: Option<ReviewLocationV1>,
    pub evidence: Vec<String>,
    pub repro: Option<String>,
    pub proposed_correction: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub affected_requirement_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<ReviewFindingActionV1>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewLocationV1 {
    pub file: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFindingActionV1 {
    Repair,
    RePlan,
    FollowUp,
    AcceptRisk,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingScopeV1 {
    Blocking,
    NonBlocking,
}
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    /// Legacy v1 spelling; new CodeReviewer outputs use `critical`.
    Blocker,
    High,
    Medium,
    Low,
    Informational,
}
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Correctness,
    SpecMismatch,
    Security,
    Requirements,
    TestGap,
    Regression,
    Scope,
    OutOfScope,
    Overengineering,
    UnnecessaryAbstraction,
    Maintainability,
    Performance,
    Documentation,
    Other,
}
