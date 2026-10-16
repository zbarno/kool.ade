use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{GateReceipt, TaskEvent, TaskPhase, TaskRecord},
};

use super::{LifecycleStore, records::load_tasks};

impl LifecycleStore<'_> {
    pub fn load_spec_delta_approval(
        &self,
        task_uid: &str,
        proposal_sha256: &str,
    ) -> Result<super::super::SpecDeltaApprovalV1, StoreError> {
        if !crate::core::lifecycle::types::is_uuid(task_uid)
            || !crate::core::lifecycle::types::is_sha256(proposal_sha256)
        {
            return Err(StoreError::MalformedState(
                "specification approval identity is invalid".into(),
            ));
        }
        let path = format!("state/lifecycle_spec_approvals/{task_uid}-{proposal_sha256}.json");
        let bytes = self.store.read(&path)?;
        let approval: super::super::SpecDeltaApprovalV1 = serde_json::from_slice(&bytes)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        approval.validate().map_err(StoreError::MalformedState)?;
        if approval.task_uid != task_uid || approval.proposal_sha256 != proposal_sha256 {
            return Err(StoreError::MalformedState(
                "specification approval record identity does not match its path".into(),
            ));
        }
        Ok(approval)
    }

    /// Reopen-safe tasks eligible for the one bounded SpecReconciler pass.
    pub fn spec_reconciliation_candidates(&self) -> Result<Vec<TaskRecord>, StoreError> {
        let mut tasks = load_tasks(self.store)?;
        tasks.retain(|task| {
            task.phase == TaskPhase::AwaitingSpecReconciliation
                && task.qa_execution.as_ref().is_some_and(|qa| {
                    qa.status == crate::core::lifecycle::QaExecutionStatusV1::Completed
                })
        });
        tasks.sort_by(|left, right| left.task_uid.cmp(&right.task_uid));
        Ok(tasks)
    }

    /// Persist a proposal that needs Manager or user resolution. It never
    /// changes the approved specification or advances the task phase.
    pub fn block_spec_reconciliation(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        block: super::super::SpecReconciliationBlockV1,
    ) -> Result<TaskRecord, StoreError> {
        let super::super::SpecReconciliationBlockV1 {
            reason,
            proposal_sha256,
            spec_sha256,
            proposal_json,
        } = block;
        let current = self.load_task(task_uid)?;
        if current.revision == expected_revision
            && current.spec_reconciliation_blocker.as_deref() == Some(reason.as_str())
            && current.spec_reconciliation_proposal_sha256.as_deref()
                == Some(proposal_sha256.as_str())
            && current.spec_reconciliation_spec_sha256.as_deref() == Some(spec_sha256.as_str())
            && current.spec_reconciliation_proposal_json == proposal_json
        {
            return Ok(current);
        }
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::SpecReconciliationBlocked {
                reason,
                proposal_sha256,
                spec_sha256,
                proposal_json,
            },
        )
    }

    pub fn retry_spec_reconciliation(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        current_spec_sha256: String,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::SpecReconciliationRetry {
                current_spec_sha256,
            },
        )
    }

    /// Record the validated no-delta result of the bounded reconciler pass.
    pub fn reconcile_specification(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        receipt: GateReceipt,
        pr_work_item_uid: &str,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::ReconcileSpec {
                receipt,
                pr_work_item_uid: pr_work_item_uid.to_owned(),
            },
        )
    }

    pub fn reconcile_subtask_specification(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        receipt: GateReceipt,
    ) -> Result<TaskRecord, StoreError> {
        self.apply_task_event(
            task_uid,
            expected_revision,
            event_uid,
            TaskEvent::SubtaskReconciled { receipt },
        )
    }
}
