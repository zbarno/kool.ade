use serde::{Deserialize, Serialize};

use super::super::types::{GateActor, is_sha256, is_uuid};
use super::has_duplicates;

/// One structured, traceable change observed while reconciling a feature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecDeltaRecordV1 {
    pub requirement_id: String,
    pub event_id: String,
    pub old_target: Option<String>,
    pub proposed_target: Option<String>,
    pub observed_current: Option<String>,
    pub classification: SpecDeltaClassificationV1,
    pub evidence_refs: Vec<String>,
    /// Decisions explicitly bound to this requirement, not reconciliation-wide sources.
    #[serde(default)]
    pub decision_refs: Vec<String>,
    /// Acceptance test references explicitly tied to this stable requirement.
    #[serde(default)]
    pub test_refs: Vec<String>,
    pub affected_task_ids: Vec<String>,
    pub needs_user_reapproval: bool,
    pub followup_ids: Vec<String>,
    #[serde(default)]
    pub operator_decision: Option<SpecDeltaOperatorDecisionV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecDeltaOperatorDecisionV1 {
    pub receipt_uid: String,
    pub decision: SpecDeltaOperatorChoiceV1,
    pub actor: GateActor,
    pub recorded_at_ms: u64,
    pub resulting_spec_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecDeltaOperatorChoiceV1 {
    Adopted,
    Declined,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecDeltaClassificationV1 {
    Clarification,
    UserDecision,
    PlanReview,
    TaskDecomposition,
    Implementation,
    ReviewFinding,
    QaFinding,
    Repair,
    PrState,
    MergeState,
    Documentation,
}

impl SpecDeltaRecordV1 {
    pub fn validate(&self) -> Result<(), String> {
        let bounded_text = |value: &Option<String>| {
            value.as_deref().is_none_or(|text| {
                !text.trim().is_empty() && text.len() <= 8192 && !text.contains('\0')
            })
        };
        if self.requirement_id.trim().is_empty()
            || self.requirement_id.len() > 128
            || self.requirement_id.contains('\0')
            || !is_uuid(&self.event_id)
            || !bounded_text(&self.old_target)
            || !bounded_text(&self.proposed_target)
            || !bounded_text(&self.observed_current)
            || self.old_target.is_none()
                && self.proposed_target.is_none()
                && self.observed_current.is_none()
            || self.evidence_refs.is_empty()
            || self.evidence_refs.len() > 64
            || self.evidence_refs.iter().any(|reference| {
                reference.trim().is_empty() || reference.len() > 512 || reference.contains('\0')
            })
            || self.test_refs.len() > 128
            || self.test_refs.iter().any(|reference| {
                reference.trim().is_empty() || reference.len() > 2048 || reference.contains('\0')
            })
            || has_duplicates(&self.test_refs)
            || self.decision_refs.len() > 128
            || self
                .decision_refs
                .iter()
                .any(|reference| !is_uuid(reference))
            || has_duplicates(&self.decision_refs)
            || has_duplicates(&self.evidence_refs)
            || self.affected_task_ids.len() > 256
            || self.affected_task_ids.iter().any(|uid| !is_uuid(uid))
            || has_duplicates(&self.affected_task_ids)
            || self.followup_ids.len() > 256
            || self.followup_ids.iter().any(|uid| !is_uuid(uid))
            || has_duplicates(&self.followup_ids)
            || self.operator_decision.as_ref().is_some_and(|decision| {
                !is_uuid(&decision.receipt_uid)
                    || decision.actor != GateActor::User
                    || decision.recorded_at_ms == 0
                    || match decision.decision {
                        SpecDeltaOperatorChoiceV1::Adopted => decision
                            .resulting_spec_sha256
                            .as_deref()
                            .is_none_or(|hash| !is_sha256(hash)),
                        SpecDeltaOperatorChoiceV1::Declined => {
                            decision.resulting_spec_sha256.is_some()
                        }
                    }
            })
        {
            return Err("specification delta is invalid".into());
        }
        Ok(())
    }
}
