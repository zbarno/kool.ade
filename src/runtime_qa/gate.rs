use crate::{
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    harness::prompt_catalog::{PromptManifest, PromptRole, validate_prompt_manifest},
};

use super::{
    QaEvidenceManifestV1, QaOutcome, QaResultV1, QaRunRequestV1, QaScenarioV1, validate_report,
};

/// Convert a validated independent QA result into a lifecycle gate receipt.
pub fn approved_gate_receipt(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    result: &QaResultV1,
    prompt: PromptManifest,
    recorded_at_unix_ms: u64,
) -> Result<GateReceipt, String> {
    request.validate()?;
    validate_report(request, scenarios, evidence, result)?;
    if result.run_id != request.qa_run_id
        || result.subject_sha != request.code_head_sha
        || !matches!(
            validate_prompt_manifest(&prompt),
            Ok(PromptRole::RuntimeQaReviewer)
        )
    {
        return Err("runtime QA gate does not match request, subject, or reviewer prompt".into());
    }
    let decision = if result.outcome == QaOutcome::Pass {
        GateDecision::Approved
    } else {
        GateDecision::ChangesRequested
    };
    let prompt_sha256 = prompt.identity_sha256();
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: request.parent_task_uid.clone(),
        subject_sha256: request.complete_diff_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision,
        recorded_at_unix_ms,
        evidence_refs: vec![
            format!("runtime-qa:{}/result", request.qa_run_id),
            format!(
                "runtime-qa:{}/evidence:{}",
                request.qa_run_id, result.evidence_manifest_sha256
            ),
        ],
        prompt_sha256: Some(prompt_sha256),
        prompt_manifest: Some(prompt),
        reviewer_run_id: Some(result.reviewer_execution_id.clone()),
        author_run_id: Some(request.author_run_id.clone()),
        reviewer_session_id: Some(result.reviewer_session_id.clone()),
        author_session_id: Some(request.author_session_id.clone()),
    };
    receipt.validate()?;
    Ok(receipt)
}
