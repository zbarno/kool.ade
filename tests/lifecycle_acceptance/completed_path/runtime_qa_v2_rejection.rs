use koolade::{
    core::lifecycle::{GateReceipt, LifecycleStore, TaskEvent},
    runtime_qa::{QaEvidenceManifestV1, QaOutcome, QaResultV1, QaRunRequestV1},
};
use sha2::Digest as _;

/// Production boundary: `LifecycleStore` rejects direct V2 and V3 completion
/// events. This crosses the QA and lifecycle stores; a unit validator cannot
/// prove direct writes stay out of the task ledger. Setup is low-cost: local
/// stores in a temporary directory, with no Docker or SSH dependency.
pub(super) struct CompletionRejectionCase<'a, 'store> {
    pub(super) lifecycle: &'a LifecycleStore<'store>,
    pub(super) task_uid: &'a str,
    pub(super) expected_revision: u64,
    pub(super) direct_legacy_event_uid: &'a str,
    pub(super) direct_current_event_uid: &'a str,
    pub(super) request: &'a QaRunRequestV1,
    pub(super) current_receipt: &'a GateReceipt,
    pub(super) evidence: &'a QaEvidenceManifestV1,
    pub(super) result: &'a QaResultV1,
}

pub(super) fn assert_direct_legacy_and_current_completion_events_rejected(
    case: CompletionRejectionCase<'_, '_>,
) {
    let CompletionRejectionCase {
        lifecycle,
        task_uid,
        expected_revision,
        direct_legacy_event_uid,
        direct_current_event_uid,
        request,
        current_receipt,
        evidence,
        result,
    } = case;
    assert_eq!(result.outcome, QaOutcome::Fail);
    assert_eq!(result.findings.len(), 1);
    assert!(result.findings[0].blocking);
    assert_eq!(evidence.code_sha, request.code_head_sha);
    let legacy_receipt = receipt_with_reviewer_role(
        request,
        current_receipt,
        koolade::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2,
    );
    let result_json = serde_json::to_vec(result).unwrap();
    let result_sha256 = format!("{:x}", sha2::Sha256::digest(&result_json));
    let evidence_sha256 = evidence.sha256().unwrap();

    assert!(
        lifecycle
            .apply_task_event(
                task_uid,
                expected_revision,
                direct_legacy_event_uid,
                TaskEvent::RuntimeQaCompleted {
                    request_id: request.qa_run_id.clone(),
                    receipt: legacy_receipt.clone(),
                    result_sha256: result_sha256.clone(),
                    evidence_manifest_sha256: evidence_sha256.clone(),
                    reviewer_execution_id: result.reviewer_execution_id.clone(),
                    reviewer_session_id: result.reviewer_session_id.clone(),
                },
            )
            .is_err()
    );
    assert!(
        lifecycle
            .apply_task_event(
                task_uid,
                expected_revision,
                direct_current_event_uid,
                TaskEvent::RuntimeQaCompleted {
                    request_id: request.qa_run_id.clone(),
                    receipt: current_receipt.clone(),
                    result_sha256,
                    evidence_manifest_sha256: evidence_sha256,
                    reviewer_execution_id: result.reviewer_execution_id.clone(),
                    reviewer_session_id: result.reviewer_session_id.clone(),
                },
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_task(task_uid).unwrap().revision,
        expected_revision
    );
}

fn receipt_with_reviewer_role(
    request: &QaRunRequestV1,
    receipt: &GateReceipt,
    role: koolade::harness::prompt_catalog::PromptRole,
) -> GateReceipt {
    let prompt = super::qa_report::reviewer_prompt(
        request,
        receipt.reviewer_run_id.as_deref().expect("reviewer run"),
        receipt
            .reviewer_session_id
            .as_deref()
            .expect("reviewer session"),
        role,
    );
    let mut receipt = receipt.clone();
    receipt.prompt_sha256 = Some(prompt.identity_sha256());
    receipt.prompt_manifest = Some(prompt);
    receipt
}
