use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRemediationV1 {
    pub schema_version: u16,
    pub feature_uid: String,
    pub snapshot_uid: String,
    pub snapshot_hash: String,
    pub review_receipt_uid: String,
    pub dispositions: Vec<PlanFindingDispositionV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanFindingDispositionV1 {
    pub finding_id: String,
    pub action: PlanFindingActionV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanFindingActionV1 {
    RepairPlan {
        rationale: String,
    },
    AskUser {
        rationale: String,
        question: String,
    },
    PromoteFollowUp {
        rationale: String,
        title: String,
    },
    ContestFinding {
        reason: String,
        evidence_refs: Vec<String>,
    },
}

impl PlanRemediationV1 {
    pub(crate) fn validate_shape(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !canonical_uuid(&self.feature_uid)
            || !canonical_uuid(&self.snapshot_uid)
            || !canonical_uuid(&self.review_receipt_uid)
            || !is_sha256(&self.snapshot_hash)
            || self.dispositions.is_empty()
        {
            return Err("plan remediation identity or schema is invalid".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for disposition in &self.dispositions {
            if disposition.finding_id.trim().is_empty()
                || !ids.insert(disposition.finding_id.as_str())
            {
                return Err("plan remediation finding IDs must be unique and nonempty".into());
            }
            match &disposition.action {
                PlanFindingActionV1::RepairPlan { rationale }
                | PlanFindingActionV1::PromoteFollowUp { rationale, .. }
                | PlanFindingActionV1::AskUser { rationale, .. }
                    if rationale.trim().is_empty() =>
                {
                    return Err("plan remediation rationale cannot be empty".into());
                }
                PlanFindingActionV1::AskUser { question, .. } if question.trim().is_empty() => {
                    return Err("AskUser requires a concrete question".into());
                }
                PlanFindingActionV1::PromoteFollowUp { title, .. } if title.trim().is_empty() => {
                    return Err("PromoteFollowUp requires a task title".into());
                }
                PlanFindingActionV1::ContestFinding {
                    reason,
                    evidence_refs,
                } if reason.trim().is_empty()
                    || evidence_refs.is_empty()
                    || evidence_refs
                        .iter()
                        .any(|reference| reference.trim().is_empty()) =>
                {
                    return Err(
                        "ContestFinding requires a reason and concrete evidence references".into(),
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "plan_remediation/tests.rs"]
mod tests;
