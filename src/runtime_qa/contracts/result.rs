use serde::{Deserialize, Serialize};

use super::{QaCaseStatus, QaEvidenceManifestV1, QaFindingV1, QaOutcome, is_git_oid};

mod review_coverage;
mod validation;
pub use review_coverage::{QaReviewCoverageStatusV1, QaReviewRequirementCoverageV1};

/// New runtime QA records use schema 4; earlier schemas remain readable for history.
pub const CURRENT_QA_RESULT_SCHEMA_VERSION: u16 = 4;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaCaseResultV1 {
    pub scenario_id: String,
    pub status: QaCaseStatus,
    pub evidence_refs: Vec<String>,
    pub observations: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaCoverageStatus {
    Observed,
    Untested,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaRequirementCoverageV1 {
    pub requirement_id: String,
    pub status: QaCoverageStatus,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaCoverageV1 {
    pub requirements: Vec<QaRequirementCoverageV1>,
    pub scenario_ids: Vec<String>,
    pub states: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaResultV1 {
    pub schema_version: u16,
    pub subject_sha: String,
    pub run_id: String,
    pub reviewer_execution_id: String,
    pub reviewer_session_id: String,
    pub outcome: QaOutcome,
    pub evidence_manifest_sha256: String,
    #[serde(default)]
    pub scenario_set_sha256: String,
    #[serde(default)]
    pub reviewed_screenshot_ids: Vec<String>,
    #[serde(default)]
    pub review_coverage: Vec<QaReviewRequirementCoverageV1>,
    pub cases: Vec<QaCaseResultV1>,
    pub coverage: QaCoverageV1,
    pub findings: Vec<QaFindingV1>,
    pub tool_versions: std::collections::BTreeMap<String, String>,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
}

pub fn validate_report(
    request: &super::QaRunRequestV1,
    scenarios: &[super::QaScenarioV1],
    manifest: &QaEvidenceManifestV1,
    result: &QaResultV1,
) -> Result<(), String> {
    super::scenario::validate_scenario_set(request, scenarios)?;
    if result.evidence_manifest_sha256 != manifest.sha256()? || !is_git_oid(&result.subject_sha) {
        return Err("runtime QA report evidence hash or subject is invalid".into());
    }
    result.validate(request, scenarios, manifest)
}

/// Validate a result that is being written or used to approve a new gate.
/// Persisted schema-1 and schema-2 reports remain readable but cannot authorize new work.
pub fn validate_current_report(
    request: &super::QaRunRequestV1,
    scenarios: &[super::QaScenarioV1],
    manifest: &QaEvidenceManifestV1,
    result: &QaResultV1,
) -> Result<(), String> {
    validate_report(request, scenarios, manifest, result)?;
    if result.schema_version != CURRENT_QA_RESULT_SCHEMA_VERSION {
        return Err("new runtime QA records must use schema version 4".into());
    }
    if request.target_id.is_none() || manifest.target_id != request.target_id {
        return Err("new runtime QA records must bind the selected runtime target".into());
    }
    Ok(())
}
