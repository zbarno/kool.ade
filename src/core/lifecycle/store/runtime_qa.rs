use sha2::Digest as _;

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, QaExecutionStatusV1, TaskEvent, TaskPhase, TaskRecord,
    },
    runtime_qa::QaRunRequestV1,
};

use super::LifecycleStore;
mod report;

impl LifecycleStore<'_> {
    /// Record a Manager-approved non-visual QA applicability assessment.
    /// The typed assessment stays in the task ledger and is never a QA pass.
    pub fn record_runtime_qa_applicability(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        proposal: crate::runtime_qa::QaApplicabilityProposalV1,
        receipt: GateReceipt,
    ) -> Result<TaskRecord, StoreError> {
        proposal.validate().map_err(StoreError::MalformedState)?;
        let assessment = &proposal.assessment;
        let reviewer_receipt = &proposal.reviewer_receipt;
        let task = self.load_task(task_uid)?;
        if task.revision != expected_revision
            || task.phase != TaskPhase::AwaitingRuntimeQa
            || proposal.task_revision != expected_revision
            || assessment.task_uid != task_uid
        {
            return Err(StoreError::MalformedState(
                "runtime QA applicability targets a stale or mismatched task revision".into(),
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
        let assessment_hash = sha256(
            &serde_json::to_vec(assessment)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
        );
        let proposal_hash = proposal
            .proposal_sha256()
            .map_err(StoreError::MalformedState)?;
        let implementation_sha = task.implementation_sha256.as_deref().ok_or_else(|| {
            StoreError::MalformedState("runtime QA applicability has no implementation hash".into())
        })?;
        if receipt.validate().is_err()
            || receipt.actor != GateActor::ApplicationPolicy
            || receipt.decision != GateDecision::NotApplicable
            || receipt.subject_uid != task.task_uid
            || receipt.subject_sha256 != implementation_sha
            || receipt.reviewer_run_id.as_deref() != Some(assessment.approved_by.as_str())
            || receipt.reviewer_session_id.as_deref()
                != Some(assessment.manager_session_id.as_str())
            || receipt.author_run_id.is_some()
            || receipt.author_session_id.is_some()
            || receipt.prompt_sha256.is_some()
            || receipt.prompt_manifest.is_some()
            || receipt.evidence_refs.len() != 3
            || !receipt.evidence_refs.iter().any(|reference| {
                reference == &format!("runtime-qa-applicability:{assessment_hash}")
            })
            || !receipt.evidence_refs.iter().any(|reference| {
                reference == &format!("gate-receipt:{}", reviewer_receipt.receipt_uid)
            })
            || !receipt.evidence_refs.iter().any(|reference| {
                reference == &format!("runtime-qa-applicability-proposal:{proposal_hash}")
            })
        {
            return Err(StoreError::MalformedState(
                "runtime QA applicability manager receipt does not match the current proposal"
                    .into(),
            ));
        }
        super::task_transition::apply_task_event(
            self.store,
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::RuntimeQaNotApplicable {
                assessment: Box::new(proposal.assessment),
                reviewer_receipt: Box::new(proposal.reviewer_receipt),
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
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}
