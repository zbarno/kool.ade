use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{GateReceipt, TaskEvent, TaskPhase, TaskRecord},
};

use super::{LifecycleStore, records::load_tasks};

impl LifecycleStore<'_> {
    /// Approved features with pending feature-scoped reconciliation work.
    pub fn feature_spec_reconciliation_candidates(
        &self,
    ) -> Result<
        Vec<(
            super::super::FeatureGoal,
            Vec<crate::core::lifecycle::SpecSyncQueueEntryV1>,
        )>,
        StoreError,
    > {
        let mut candidates = Vec::new();
        for path in self.store.list_record_paths("state/lifecycle_features")? {
            let feature_uid = path
                .strip_prefix("state/lifecycle_features/")
                .and_then(|path| path.strip_suffix(".json"))
                .ok_or_else(|| StoreError::MalformedState("feature path is invalid".into()))?;
            let feature = self.load_feature(feature_uid)?;
            if feature.plan_snapshot_uid.is_none() || feature.approved_plan_sha256.is_none() {
                continue;
            }
            let queue = self.list_spec_sync_queue(feature_uid, false)?;
            let feature_events = queue
                .into_iter()
                .filter(|entry| {
                    entry.source_task_uid.is_none()
                        && matches!(
                            entry.trigger,
                            crate::core::lifecycle::SpecSyncTriggerKindV1::FeatureClarification
                                | crate::core::lifecycle::SpecSyncTriggerKindV1::UserDecision
                                | crate::core::lifecycle::SpecSyncTriggerKindV1::PlanReview
                                | crate::core::lifecycle::SpecSyncTriggerKindV1::TaskGeneration
                                | crate::core::lifecycle::SpecSyncTriggerKindV1::ManualRefresh
                        )
                })
                .collect::<Vec<_>>();
            if let Some(revision) = self.load_spec_revision(feature_uid)?
                && revision.reconciliation_status
                    == crate::core::lifecycle::SpecReconciliationStatusV1::NeedsHumanDecision
                && feature_events
                    .iter()
                    .all(|entry| revision.origin_events.contains(&entry.source_event_uid))
            {
                continue;
            }
            if !feature_events.is_empty() {
                candidates.push((feature, feature_events));
            }
        }
        candidates.sort_by(|(left, _), (right, _)| left.feature_uid.cmp(&right.feature_uid));
        Ok(candidates)
    }

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
        let mut pending_pr_tasks = std::collections::HashSet::new();
        for task in tasks
            .iter()
            .filter(|task| task.phase == TaskPhase::Done && task.pr_work_item_uid.is_some())
        {
            let has_pending_pr = self
                .list_spec_sync_queue(&task.feature_uid, false)?
                .iter()
                .any(|entry| {
                    entry.source_task_uid.as_deref() == Some(task.task_uid.as_str())
                        && entry.trigger
                            == crate::core::lifecycle::SpecSyncTriggerKindV1::PullRequest
                });
            if has_pending_pr {
                pending_pr_tasks.insert(task.task_uid.clone());
            }
        }
        tasks.retain(|task| {
            let qa_complete = task.qa_execution.as_ref().is_some_and(|qa| {
                qa.status == crate::core::lifecycle::QaExecutionStatusV1::Completed
            });
            let task_pass = task.phase == TaskPhase::AwaitingSpecReconciliation && qa_complete;
            let pr_refresh = pending_pr_tasks.contains(&task.task_uid) && qa_complete;
            task_pass || pr_refresh
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
