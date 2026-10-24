use super::super::QaOutcome;
use super::{
    QaEvidenceManifestV1, QaResultV1, QaRunRequestV1, RuntimeQaStore, validate_current_report,
};
use super::{create_or_match_json, read_json, safe_uuid};

impl RuntimeQaStore {
    /// Persist the exact reviewer receipt generated for a validated run.
    /// The receipt remains a distinct immutable record so interrupted runs
    /// without a complete report cannot be advanced through lifecycle.
    pub fn save_gate_receipt(
        &self,
        receipt: &crate::core::lifecycle::GateReceipt,
    ) -> Result<(), String> {
        receipt.validate()?;
        let run = receipt
            .evidence_refs
            .iter()
            .find_map(|reference| {
                reference
                    .strip_prefix("runtime-qa:")
                    .and_then(|rest| rest.split_once("/result"))
                    .filter(|(_, suffix)| suffix.is_empty())
                    .map(|(run, _)| run)
            })
            .ok_or_else(|| "runtime QA receipt does not reference a run result".to_owned())?;
        let (request, scenarios, evidence, result) = self.load_run(run)?;
        validate_current_report(&request, &scenarios, &evidence, &result)?;
        validate_gate_receipt(receipt, &request, &evidence, &result)?;
        create_or_match_json(
            &self.root,
            &self.root.join(safe_uuid(run)?).join("gate-receipt.json"),
            receipt,
        )
    }

    /// Load a persisted reviewer receipt only when it still matches the
    /// complete, revalidated report and exact author/reviewer identities.
    pub fn load_gate_receipt(
        &self,
        qa_run_id: &str,
    ) -> Result<crate::core::lifecycle::GateReceipt, String> {
        let (request, _, evidence, result) = self.load_run(qa_run_id)?;
        let receipt = read_json(
            &self.root,
            &self
                .root
                .join(safe_uuid(qa_run_id)?)
                .join("gate-receipt.json"),
        )?;
        validate_gate_receipt(&receipt, &request, &evidence, &result)?;
        Ok(receipt)
    }
}

fn validate_gate_receipt(
    receipt: &crate::core::lifecycle::GateReceipt,
    request: &QaRunRequestV1,
    evidence: &QaEvidenceManifestV1,
    result: &QaResultV1,
) -> Result<(), String> {
    use crate::core::lifecycle::{GateActor, GateDecision};

    receipt.validate()?;
    let expected_decision = match result.outcome {
        QaOutcome::Pass => GateDecision::Approved,
        QaOutcome::Fail => GateDecision::ChangesRequested,
        QaOutcome::Incomplete | QaOutcome::InfrastructureBlocked => {
            return Err("incomplete runtime QA cannot have a reviewer gate receipt".into());
        }
    };
    let evidence_hash = evidence.sha256()?;
    let prompt_matches_result_version = if result.schema_version >= 3 {
        receipt.prompt_role()
            == Some(crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3)
    } else {
        matches!(
            receipt.prompt_role(),
            Some(
                crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer
                    | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2
            )
        )
    };
    if receipt.actor != GateActor::IndependentReviewer
        || receipt.decision != expected_decision
        || receipt.subject_uid != request.parent_task_uid
        || receipt.subject_sha256 != request.complete_diff_hash
        || receipt.author_run_id.as_deref() != Some(request.author_run_id.as_str())
        || receipt.author_session_id.as_deref() != Some(request.author_session_id.as_str())
        || receipt.reviewer_run_id.as_deref() != Some(result.reviewer_execution_id.as_str())
        || receipt.reviewer_session_id.as_deref() != Some(result.reviewer_session_id.as_str())
        || !prompt_matches_result_version
        || !receipt
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("runtime-qa:{}/result", request.qa_run_id))
        || !receipt.evidence_refs.iter().any(|reference| {
            reference == &format!("runtime-qa:{}/evidence:{evidence_hash}", request.qa_run_id)
        })
        || (result.schema_version == super::super::CURRENT_QA_RESULT_SCHEMA_VERSION
            && !receipt.evidence_refs.iter().any(|reference| {
                reference
                    == &format!(
                        "runtime-qa:{}/scenarios:{}",
                        request.qa_run_id, result.scenario_set_sha256
                    )
            }))
    {
        return Err("runtime QA reviewer receipt does not match its persisted run".into());
    }
    Ok(())
}
