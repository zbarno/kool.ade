use sha2::Digest as _;

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{GateDecision, GateReceipt, TaskEvent, TaskPhase, TaskRecord},
    runtime_qa::{QaOutcome, RuntimeQaStore},
};

use super::LifecycleStore;

impl LifecycleStore<'_> {
    /// Record only a report already persisted and fully revalidated by RuntimeQaStore.
    /// Infrastructure failures remain blocked and cannot advance to spec reconciliation.
    pub fn record_runtime_qa_report(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        qa_store: &RuntimeQaStore,
        receipt: Option<GateReceipt>,
    ) -> Result<TaskRecord, StoreError> {
        let task = self.load_task(task_uid)?;
        if task.revision != expected_revision || task.phase != TaskPhase::AwaitingRuntimeQa {
            return Err(StoreError::MalformedState(
                "runtime QA report targets a stale or ineligible task".into(),
            ));
        }
        let request = task
            .qa_execution
            .as_ref()
            .and_then(|state| state.request.as_ref())
            .ok_or_else(|| StoreError::MalformedState("runtime QA request is missing".into()))?;
        let (persisted_request, _, evidence, result) = qa_store
            .load_run(&request.qa_run_id)
            .map_err(StoreError::MalformedState)?;
        if &persisted_request != request {
            return Err(StoreError::MalformedState(
                "runtime QA report does not match the queued request".into(),
            ));
        }
        let encoded = serde_json::to_vec(&result)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        let result_sha256 = format!("{:x}", sha2::Sha256::digest(encoded));
        let evidence_manifest_sha256 = evidence.sha256().map_err(StoreError::MalformedState)?;
        match result.outcome {
            QaOutcome::Pass | QaOutcome::Fail => {
                let receipt = receipt.ok_or_else(|| {
                    StoreError::MalformedState("completed QA report has no review receipt".into())
                })?;
                let expected_decision = if result.outcome == QaOutcome::Pass {
                    GateDecision::Approved
                } else {
                    GateDecision::ChangesRequested
                };
                if receipt.decision != expected_decision
                    || receipt.reviewer_run_id.as_deref()
                        != Some(result.reviewer_execution_id.as_str())
                    || receipt.reviewer_session_id.as_deref()
                        != Some(result.reviewer_session_id.as_str())
                    || !receipt.evidence_refs.iter().any(|reference| {
                        reference
                            == &format!(
                                "runtime-qa:{}/evidence:{evidence_manifest_sha256}",
                                request.qa_run_id
                            )
                    })
                {
                    return Err(StoreError::MalformedState(
                        "runtime QA receipt does not match the persisted result".into(),
                    ));
                }
                self.apply_task_event(
                    task_uid,
                    expected_revision,
                    event_uid,
                    TaskEvent::RuntimeQaCompleted {
                        request_id: request.qa_run_id.clone(),
                        receipt,
                        result_sha256,
                        evidence_manifest_sha256,
                        reviewer_execution_id: result.reviewer_execution_id,
                        reviewer_session_id: result.reviewer_session_id,
                    },
                )
            }
            QaOutcome::Incomplete | QaOutcome::InfrastructureBlocked => {
                if receipt.is_some() {
                    return Err(StoreError::MalformedState(
                        "incomplete or infrastructure-blocked QA cannot have a passing receipt"
                            .into(),
                    ));
                }
                let reason = format!("runtime QA outcome: {:?}", result.outcome);
                self.apply_task_event(
                    task_uid,
                    expected_revision,
                    event_uid,
                    TaskEvent::RuntimeQaBlocked {
                        request_id: request.qa_run_id.clone(),
                        reason,
                        result_sha256: Some(result_sha256),
                        evidence_manifest_sha256: Some(evidence_manifest_sha256),
                    },
                )
            }
        }
    }
}
