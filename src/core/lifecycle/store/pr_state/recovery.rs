use super::*;

impl LifecycleStore<'_> {
    /// Persist a publication-only blocker in the same transaction as its audit
    /// event. The parent Done record and its reviewed receipts are untouched.
    pub fn record_pr_publication_failure(
        &self,
        work_item_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        expected_lease_epoch: Option<u64>,
        reason: &str,
    ) -> Result<PullRequestPreparation, StoreError> {
        let mut record = read(self.store, work_item_uid)?;
        let task = self.load_task(&record.task_uid)?;
        if record.revision != expected_revision
            || expected_lease_epoch.is_some_and(|epoch| epoch != record.publication_lease_epoch())
            || (record.status == PullRequestPreparationStatus::Publishing
                && expected_lease_epoch != Some(record.publication_lease_epoch()))
            || task.phase != TaskPhase::Done
            || task.pr_work_item_uid.as_deref() != Some(work_item_uid)
            || !matches!(
                record.status,
                PullRequestPreparationStatus::AwaitingAuthorization
                    | PullRequestPreparationStatus::Publishing
                    | PullRequestPreparationStatus::NeedsAttention
            )
        {
            return Err(StoreError::MalformedState(
                "publication failure is stale or does not belong to current Done work".into(),
            ));
        }
        // The record has a byte bound; the shared clipping helper counts
        // characters. 1,000 characters also bounds four-byte UTF-8 diagnostics.
        let reason = crate::core::context_build::clip(
            &crate::error::redact_secrets(reason).replace('\0', "\\0"),
            1000,
        );
        let recovery = record
            .recovery
            .get_or_insert(super::super::PrPublicationRecoveryV1 {
                attempt: u64::from(record.publication_authorization.is_some()),
                last_failure: None,
            });
        if record.status == PullRequestPreparationStatus::NeedsAttention
            && recovery.last_failure.as_deref() == Some(reason.as_str())
        {
            return Ok(record);
        }
        recovery.last_failure = Some(reason);
        record.status = PullRequestPreparationStatus::NeedsAttention;
        persist(
            self.store,
            record,
            expected_revision,
            event_uid,
            LifecycleActor::ApplicationPolicy,
            None,
        )
    }
}
