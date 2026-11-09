use serde::{Deserialize, Serialize};

use sha2::Digest as _;

use super::{QaExecutionTargetV1, QaRunRequestV1, is_sha256, is_uuid};

/// Persisted user approval for launching this exact built task in a disposable
/// Computer Use guest. It is separate from the guest session's Human consent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaComputerBuildApprovalV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_uid: String,
    pub task_revision: u64,
    pub revision_sha: String,
    pub approved_spec_hash: String,
    pub app_target_id: String,
    pub execution_recipe_id: String,
    pub execution_recipe_sha256: String,
    pub build_receipt_sha256: String,
    pub request_sha256: String,
    pub scenario_proposal_sha256: String,
    pub scenario_sha256: String,
    pub recorded_at_unix_ms: u64,
    pub user_approval: crate::core::lifecycle::GateReceipt,
}

impl QaComputerBuildApprovalV1 {
    pub fn approval_sha256(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("Computer QA approval could not be encoded: {error}"))?;
        Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
    }

    pub fn validate_for(
        &self,
        request: &QaRunRequestV1,
        task_revision: u64,
        execution_target: QaExecutionTargetV1,
    ) -> Result<(), String> {
        let expected_proposal = format!(
            "runtime-qa:{}/proposal:{}:task-revision:{}",
            request.qa_run_id, self.scenario_proposal_sha256, task_revision
        );
        let expected_target = format!(
            "runtime-qa:{}/execution-target:computer-use-guest",
            request.qa_run_id
        );
        let expected_approved_scenarios =
            format!("runtime-qa:{}/approved-scenarios", request.qa_run_id);
        let request_sha256 = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(request).map_err(|error| error.to_string())?)
        );
        let receipt = &self.user_approval;
        receipt.validate()?;
        if execution_target != QaExecutionTargetV1::ComputerUseGuest
            || self.schema_version != 1
            || self.qa_run_id != request.qa_run_id
            || self.task_uid != request.parent_task_uid
            || self.task_revision != task_revision
            || self.revision_sha != request.code_head_sha
            || self.approved_spec_hash != request.approved_spec_hash
            || self.app_target_id != request.target_id.as_deref().unwrap_or_default()
            || self.execution_recipe_id != request.startup_recipe_ref
            || self.execution_recipe_sha256 != request.startup_recipe_sha256
            || self.build_receipt_sha256 != request.build_artifact_sha256
            || self.request_sha256 != request_sha256
            || !is_sha256(&self.scenario_proposal_sha256)
            || !is_sha256(&self.scenario_sha256)
            || self.recorded_at_unix_ms == 0
            || receipt.subject_uid != self.task_uid
            || receipt.subject_sha256 != self.scenario_sha256
            || receipt.actor != crate::core::lifecycle::GateActor::User
            || receipt.decision != crate::core::lifecycle::GateDecision::Approved
            || receipt.recorded_at_unix_ms != self.recorded_at_unix_ms
            || !receipt
                .evidence_refs
                .iter()
                .any(|reference| reference == &expected_proposal)
            || !receipt
                .evidence_refs
                .iter()
                .any(|reference| reference == &expected_target)
            || !receipt
                .evidence_refs
                .iter()
                .any(|reference| reference == &expected_approved_scenarios)
            || receipt.reviewer_run_id.is_some()
            || receipt.reviewer_session_id.is_some()
            || receipt.author_run_id.is_some()
            || receipt.author_session_id.is_some()
            || !is_uuid(&self.task_uid)
        {
            return Err(
                "Computer QA build approval is stale or does not bind the exact task build".into(),
            );
        }
        Ok(())
    }
}
