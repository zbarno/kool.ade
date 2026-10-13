use serde::{Deserialize, Serialize};

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
    pub code_sha: String,
    pub approved_spec_hash: String,
    pub applicability: QaApplicability,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub approved_by: String,
    pub recorded_at_unix_ms: u64,
}

impl QaApplicabilityAssessmentV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.task_uid)
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
            || !is_uuid(&self.approved_by)
            || self.recorded_at_unix_ms == 0
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
