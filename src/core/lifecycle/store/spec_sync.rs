use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{FeatureGoal, SpecReconciliationStatusV1, SpecRevisionDraftV1},
};

use super::LifecycleStore;

/// Reject lifecycle transitions that would need to reconcile a missing
/// product specification before any of their event or task writes begin.
pub(super) fn require_product_specification(
    lifecycle: &LifecycleStore<'_>,
) -> Result<(), StoreError> {
    crate::core::lifecycle::product_spec::current_product_hash(lifecycle.store)?;
    Ok(())
}

impl LifecycleStore<'_> {
    /// Gate confirmation and task generation on the exact current product hash.
    pub fn ensure_product_specification_current(
        &self,
        feature_uid: &str,
    ) -> Result<(), StoreError> {
        let current_hash = crate::core::lifecycle::product_spec::current_product_hash(self.store)?;
        let revision = self.load_spec_revision(feature_uid)?.ok_or_else(|| {
            StoreError::MalformedState(
                "product specification has no recorded lifecycle baseline".into(),
            )
        })?;
        if current_hash != revision.content_hash {
            return Err(StoreError::MalformedState(
                "product specification drifted after its last lifecycle reconciliation".into(),
            ));
        }
        if !matches!(
            revision.reconciliation_status,
            SpecReconciliationStatusV1::Synchronized
                | SpecReconciliationStatusV1::OutstandingNonblockingFindings
        ) {
            return Err(StoreError::MalformedState(
                "product specification has unresolved reconciliation findings".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn ensure_task_specification_current(
        &self,
        task: &crate::core::lifecycle::TaskRecord,
    ) -> Result<(), StoreError> {
        self.ensure_product_specification_current(&task.feature_uid)?;
        if self
            .list_spec_sync_queue(&task.feature_uid, false)?
            .iter()
            .any(|entry| {
                entry.source_task_uid.as_deref().is_none_or(|uid| {
                    uid == task.task_uid || task.subtask_uids.iter().any(|child| child == uid)
                })
            })
        {
            return Err(StoreError::MalformedState(
                "task specification has unresolved lifecycle trigger(s) in its completion scope"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn ensure_no_pending_spec_triggers(&self, feature_uid: &str) -> Result<(), StoreError> {
        let pending = self.list_spec_sync_queue(feature_uid, false)?;
        if !pending.is_empty() {
            return Err(StoreError::MalformedState(format!(
                "product specification has {} unresolved lifecycle trigger(s)",
                pending.len()
            )));
        }
        Ok(())
    }
}

/// Record deterministic plan evidence before generated tasks can execute. This
/// advances specification bookkeeping without claiming implementation exists.
pub(super) fn record_approved_plan_revision(
    lifecycle: &LifecycleStore<'_>,
    feature: &FeatureGoal,
    origin_events: &[String],
) -> Result<(), StoreError> {
    let content_hash = crate::core::lifecycle::product_spec::current_product_hash(lifecycle.store)?;
    let current = lifecycle.load_spec_revision(&feature.feature_uid)?;
    let expected_sequence = current.as_ref().map_or(0, |revision| revision.seq);
    lifecycle.append_spec_revision(
        &feature.feature_uid,
        expected_sequence,
        SpecRevisionDraftV1 {
            no_change_reason: Some(
                "Application policy recorded the approved plan and task-graph bookkeeping against the unchanged rendered product specification; this transition establishes no implementation or product-text change.".into(),
            ),
            content_hash,
            source_revision: feature.revision,
            produced_by_run_id: None,
            origin_events: origin_events.to_vec(),
            decision_refs: feature.decision_refs.clone(),
            delta_approval_refs: Vec::new(),
            approved_target_hash: feature.approved_plan_sha256.clone(),
            implementation_head_sha: None,
            reconciliation_status: SpecReconciliationStatusV1::Synchronized,
            outcome_reason: None,
            deltas: Vec::new(),
        },
    )?;
    Ok(())
}
