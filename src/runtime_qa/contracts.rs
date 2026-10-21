use serde::{Deserialize, Serialize};
use sha2::Digest as _;

mod browser;
mod evidence;
mod finding;
mod linux_desktop;
mod request;
mod result;
mod scenario;

pub use browser::*;
pub use evidence::*;
pub use finding::*;
pub use linux_desktop::*;
pub use request::*;
pub use result::*;
pub use scenario::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaOutcome {
    Pass,
    Fail,
    Incomplete,
    InfrastructureBlocked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaCaseStatus {
    Passed,
    Failed,
    Skipped,
    Blocked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaApplicability {
    Required,
    NotApplicable,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaApplicabilityAssessmentV1 {
    pub schema_version: u16,
    pub task_uid: String,
    pub target_id: String,
    pub app_surface: crate::project_runtime::AppSurface,
    pub code_sha: String,
    pub approved_spec_hash: String,
    pub applicability: QaApplicability,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    /// Independent QA reviewer whose read-only assessment found the status.
    pub reviewer_execution_id: String,
    pub reviewer_session_id: String,
    /// Manager run that explicitly approved the assessment.
    pub approved_by: String,
    pub manager_session_id: String,
    pub recorded_at_unix_ms: u64,
}

/// Independent review evidence awaiting an explicit Manager decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaApplicabilityProposalV1 {
    pub schema_version: u16,
    pub task_revision: u64,
    pub assessment: QaApplicabilityAssessmentV1,
    pub reviewer_verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
    pub reviewer_receipt: crate::core::lifecycle::GateReceipt,
}

impl QaApplicabilityProposalV1 {
    pub fn proposal_sha256(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.assessment.validate()?;
        let assessment_bytes =
            serde_json::to_vec(&self.assessment).map_err(|error| error.to_string())?;
        let assessment_hash = format!("{:x}", sha2::Sha256::digest(assessment_bytes));
        let receipt = &self.reviewer_receipt;
        receipt.validate()?;
        let required = "VISUAL_QA_APPLICABILITY";
        if self.schema_version != 1
            || self.reviewer_verdict.subject_hash != assessment_hash
            || !self.reviewer_verdict.pass
            || !self.reviewer_verdict.findings.is_empty()
            || !self.reviewer_verdict.untested.is_empty()
            || !self.reviewer_verdict.coverage.iter().any(|coverage| {
                coverage.requirement == required
                    && coverage.status == crate::harness::prompt_catalog::CoverageStatus::Pass
                    && !coverage.evidence.is_empty()
            })
            || receipt.actor != crate::core::lifecycle::GateActor::IndependentReviewer
            || receipt.decision != crate::core::lifecycle::GateDecision::Approved
            || receipt.subject_uid != self.assessment.task_uid
            || receipt.subject_sha256 != assessment_hash
            || receipt.reviewer_run_id.as_deref()
                != Some(self.assessment.reviewer_execution_id.as_str())
            || receipt.reviewer_session_id.as_deref()
                != Some(self.assessment.reviewer_session_id.as_str())
            || receipt.author_run_id.as_deref() != Some(self.assessment.approved_by.as_str())
            || receipt.author_session_id.as_deref()
                != Some(self.assessment.manager_session_id.as_str())
            || !matches!(
                receipt.prompt_role(),
                Some(crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2)
            )
            || !receipt.evidence_refs.iter().any(|reference| {
                reference == &format!("runtime-qa-applicability-assessment:{assessment_hash}")
            })
        {
            return Err(
                "runtime QA applicability proposal lacks exact independent reviewer signoff".into(),
            );
        }
        Ok(())
    }
}

impl QaApplicabilityAssessmentV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.task_uid)
            || !valid_text(&self.target_id, 128)
            || !is_git_oid(&self.code_sha)
            || !is_sha256(&self.approved_spec_hash)
            || self.reason.trim().is_empty()
            || self.reason.len() > 4096
            || self.evidence_refs.is_empty()
            || self.evidence_refs.len() > 128
            || self
                .evidence_refs
                .iter()
                .any(|item| !valid_text(item, 2048))
            || !is_uuid(&self.reviewer_execution_id)
            || !is_uuid(&self.reviewer_session_id)
            || self.reviewer_execution_id == self.reviewer_session_id
            || !is_uuid(&self.approved_by)
            || !is_uuid(&self.manager_session_id)
            || self.approved_by == self.manager_session_id
            || self.reviewer_execution_id == self.approved_by
            || self.reviewer_session_id == self.manager_session_id
            || self.recorded_at_unix_ms == 0
            || matches!(
                (self.applicability, self.app_surface),
                (
                    QaApplicability::NotApplicable,
                    crate::project_runtime::AppSurface::UnsupportedTarget
                ) | (
                    QaApplicability::Unsupported,
                    crate::project_runtime::AppSurface::HeadlessService
                ) | (
                    QaApplicability::Required,
                    crate::project_runtime::AppSurface::UnsupportedTarget
                )
            )
            || (self.applicability == QaApplicability::NotApplicable
                && self.app_surface != crate::project_runtime::AppSurface::HeadlessService)
            || (self.applicability == QaApplicability::Unsupported
                && self.app_surface != crate::project_runtime::AppSurface::UnsupportedTarget)
        {
            return Err("runtime QA applicability assessment is invalid".into());
        }
        Ok(())
    }
}

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn is_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.contains('\0')
}
