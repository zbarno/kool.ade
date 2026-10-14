use serde::{Deserialize, Serialize};

use super::{
    GateActor, GateDecision, GateReceipt,
    types::{is_sha256, is_uuid},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecDeltaApprovalV1 {
    pub schema_version: u16,
    pub task_uid: String,
    pub expected_task_revision: u64,
    pub proposal_sha256: String,
    pub base_spec_sha256: String,
    pub resulting_spec_sha256: String,
    pub receipt: GateReceipt,
}

/// A blocked proposal and the approved specification identity it was checked against.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecReconciliationBlockV1 {
    pub reason: String,
    pub proposal_sha256: String,
    pub spec_sha256: String,
    pub proposal_json: Option<String>,
}

impl SpecDeltaApprovalV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.task_uid)
            || !is_sha256(&self.proposal_sha256)
            || !is_sha256(&self.base_spec_sha256)
            || !is_sha256(&self.resulting_spec_sha256)
            || self.receipt.actor != GateActor::User
            || self.receipt.decision != GateDecision::Approved
            || self.receipt.subject_uid != self.task_uid
            || self.receipt.subject_sha256 != self.proposal_sha256
            || self.receipt.validate().is_err()
        {
            return Err("specification delta approval is invalid".into());
        }
        Ok(())
    }
}
