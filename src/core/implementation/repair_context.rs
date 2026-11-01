use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

/// Application-owned scope for one Manager-assigned code-review or Runtime QA repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagerRepairContextV1 {
    pub work_id: String,
    pub task_uid: String,
    pub finding_id: String,
    #[serde(default)]
    pub finding_fingerprint: Option<String>,
    pub reviewed_subject_hash: String,
    pub reviewed_head_sha: String,
    pub evidence_ref: String,
    pub manager_reason: String,
    pub finding_record: String,
}

impl ManagerRepairContextV1 {
    pub(crate) fn is_runtime_qa(&self) -> bool {
        self.evidence_ref.starts_with("runtime-qa-finding:")
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        let evidence_ref_valid = if self.is_runtime_qa() {
            let finding = self
                .evidence_ref
                .strip_prefix("runtime-qa-finding:")
                .unwrap_or_default();
            uuid::Uuid::parse_str(finding).is_ok()
                && uuid::Uuid::parse_str(finding)
                    .is_ok_and(|id| id.hyphenated().to_string() == finding)
        } else {
            self.evidence_ref.starts_with("code-review-finding:")
                && self.evidence_ref.len() == 44
                && self.evidence_ref[20..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        };
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.work_id).is_ok()
                && uuid::Uuid::parse_str(&self.task_uid).is_ok()
                && !self.finding_id.trim().is_empty()
                && self.finding_id.len() <= 128
                && self.finding_fingerprint.as_deref().is_none_or(is_sha256)
                && is_sha256(&self.reviewed_subject_hash)
                && is_git_oid(&self.reviewed_head_sha)
                && evidence_ref_valid
                && self.manager_reason.trim().len() >= 16
                && self.manager_reason.len() <= 2048
                && !self.finding_record.trim().is_empty()
                && self.finding_record.len() <= 8192,
            "Manager repair context is invalid or exceeds its limits"
        );
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
