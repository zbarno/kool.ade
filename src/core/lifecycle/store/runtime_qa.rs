use sha2::Digest as _;

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        GateDecision, GateReceipt, QaExecutionStatusV1, TaskEvent, TaskPhase, TaskRecord,
    },
    runtime_qa::{QaOutcome, QaRunRequestV1, RuntimeQaStore},
};

use super::LifecycleStore;

impl LifecycleStore<'_> {
    /// Record a Manager-approved non-visual QA applicability assessment.
    /// The typed assessment stays in the task ledger and is never a QA pass.
    pub fn record_runtime_qa_applicability(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        assessment: crate::runtime_qa::QaApplicabilityAssessmentV1,
        reviewer_receipt: GateReceipt,
        receipt: GateReceipt,
    ) -> Result<TaskRecord, StoreError> {
        let task = self.load_task(task_uid)?;
        if task.revision != expected_revision {
            return Err(StoreError::MalformedState(
                "runtime QA applicability targets a stale task revision".into(),
            ));
        }
        let manifest = crate::project_runtime::ProjectRuntimeStore::new(self.store)
            .load_manifest()?
            .ok_or_else(|| {
                StoreError::MalformedState(
                    "runtime QA applicability requires a persisted project target manifest".into(),
                )
            })?;
        let owning_task_id = task.planning_item_uid.as_deref().unwrap_or(&task.task_uid);
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == assessment.target_id);
        let manifest_hash = sha256(
            &serde_json::to_vec(&manifest)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
        );
        if !manifest.is_active_target(owning_task_id, &assessment.target_id)
            || target.is_none_or(|target| {
                target.app_surface != crate::project_runtime::AppSurface::HeadlessService
                    || target.repository_id != manifest.repository_id
            })
            || assessment.app_surface != crate::project_runtime::AppSurface::HeadlessService
            || manifest.source_sha != assessment.code_sha
            || task.reviewed_code_head_sha.as_deref() != Some(&manifest.source_sha)
            || assessment.approved_spec_hash != task.approved_plan_sha256
            || !assessment
                .evidence_refs
                .iter()
                .any(|reference| reference == &format!("runtime-manifest:{manifest_hash}"))
        {
            return Err(StoreError::MalformedState(
                "runtime QA applicability target or manifest does not match the reviewed task"
                    .into(),
            ));
        }
        let target = target.ok_or_else(|| {
            StoreError::MalformedState("headless target disappeared during validation".into())
        })?;
        let runtime_store = crate::project_runtime::ProjectRuntimeStore::new(self.store);
        let service_receipts = runtime_store
            .headless_service_test_receipts(owning_task_id, target, &assessment.code_sha)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        if service_receipts.iter().any(|receipt| {
            !assessment
                .evidence_refs
                .iter()
                .any(|reference| reference == &format!("runtime-receipt:{}", receipt.receipt_id))
        }) {
            return Err(StoreError::MalformedState(
                "headless applicability review omitted declared build or test evidence".into(),
            ));
        }
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::RuntimeQaNotApplicable {
                assessment: Box::new(assessment),
                reviewer_receipt: Box::new(reviewer_receipt),
                receipt: Box::new(receipt),
            },
        )
    }

    /// Queue a request only after the exact code revision passed independent review.
    pub fn queue_runtime_qa(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        request: QaRunRequestV1,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::RuntimeQaRequested(request),
        )
    }

    /// Reopen-safe list of tasks that still need a concrete QA request or retry.
    pub fn runtime_qa_candidates(&self) -> Result<Vec<TaskRecord>, StoreError> {
        let mut tasks = super::records::load_tasks(self.store)?;
        tasks.retain(|task| {
            task.phase == TaskPhase::AwaitingRuntimeQa
                && task.qa_execution.as_ref().is_some_and(|state| {
                    matches!(
                        state.status,
                        QaExecutionStatusV1::Pending
                            | QaExecutionStatusV1::Running
                            | QaExecutionStatusV1::Blocked
                    )
                })
        });
        tasks.sort_by(|left, right| left.task_uid.cmp(&right.task_uid));
        Ok(tasks)
    }

    /// Pending-only view for callers that should not re-evaluate blocked runs.
    pub fn pending_runtime_qa(&self) -> Result<Vec<TaskRecord>, StoreError> {
        Ok(self
            .runtime_qa_candidates()?
            .into_iter()
            .filter(|task| {
                task.qa_execution
                    .as_ref()
                    .is_some_and(|state| state.status == QaExecutionStatusV1::Pending)
            })
            .collect())
    }

    /// Atomically claim the exact pending QA request after its per-run lock is
    /// held. A running request may be resumed after a process restart, but a
    /// different request or task revision can never inherit this claim.
    pub fn claim_runtime_qa_worker(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        request: &QaRunRequestV1,
    ) -> Result<TaskRecord, StoreError> {
        let task = self.load_task(task_uid)?;
        if task.revision != expected_revision || task.phase != TaskPhase::AwaitingRuntimeQa {
            return Err(StoreError::MalformedState(
                "runtime QA dispatch claim targets a stale or ineligible task".into(),
            ));
        }
        let execution = task
            .qa_execution
            .as_ref()
            .ok_or_else(|| StoreError::MalformedState("runtime QA state is missing".into()))?;
        if execution.request.as_ref() != Some(request) {
            return Err(StoreError::MalformedState(
                "runtime QA dispatch claim does not match the queued request".into(),
            ));
        }
        match execution.status {
            QaExecutionStatusV1::Pending => self.apply_task_event(
                task_uid,
                expected_revision,
                event_uid,
                TaskEvent::RuntimeQaWorkerStarted {
                    request_id: request.qa_run_id.clone(),
                },
            ),
            QaExecutionStatusV1::Running => Ok(task),
            _ => Err(StoreError::MalformedState(
                "runtime QA dispatch claim requires a pending or recoverable run".into(),
            )),
        }
    }

    /// Fail closed when the selected app target has no supported isolated QA driver.
    pub fn block_runtime_qa(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        request_id: &str,
        reason: String,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::RuntimeQaBlocked {
                request_id: request_id.to_owned(),
                reason,
                result_sha256: None,
                evidence_manifest_sha256: None,
            },
        )
    }

    /// Record why a safe QA request could not be formed. This leaves the task
    /// blocked without inventing a run ID, recipe, build, or evidence receipt.
    pub fn block_runtime_qa_unavailable(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        reason: String,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::RuntimeQaUnavailable { reason },
        )
    }

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

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}
