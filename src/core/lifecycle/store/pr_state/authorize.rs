use super::*;

pub(in crate::core::lifecycle::store) fn authorize(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    receipt: &GateReceipt,
    scope: Option<&crate::core::lifecycle::store::task_transition::PrPublicationScopeV1>,
) -> Result<PullRequestPreparation, StoreError> {
    authorize_before_commit(
        store,
        work_item_uid,
        expected_revision,
        event_uid,
        receipt,
        scope,
        || {},
    )
}

pub(in crate::core::lifecycle) fn authorize_before_commit(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    receipt: &GateReceipt,
    scope: Option<&crate::core::lifecycle::store::task_transition::PrPublicationScopeV1>,
    before_commit: impl FnOnce(),
) -> Result<PullRequestPreparation, StoreError> {
    let store_revision = store.revision()?;
    receipt.validate().map_err(StoreError::MalformedState)?;
    let mut record = read(store, work_item_uid)?;
    let lifecycle = LifecycleStore::new(store);
    if record.publication_authorization.as_ref() == Some(receipt)
        && matches!(
            record.status,
            PullRequestPreparationStatus::Publishing | PullRequestPreparationStatus::Opened
        )
    {
        gate::check_idempotent_retry(&lifecycle, &record)?;
        if scope.is_some_and(|scope| record.publication_scope.as_ref() != Some(scope)) {
            return Err(StoreError::MalformedState(
                "publication retry scope differs from the frozen authorization".into(),
            ));
        }
        before_commit();
        // The locked revision check is the retry attempt's linearization point.
        store.transaction_with_revision_and_record_revisions(&[], Some(&store_revision), &[])?;
        return Ok(record);
    }
    gate::check(&lifecycle, &record)?;
    let task = lifecycle.load_task(&record.task_uid)?;
    let scope_is_current = scope
        .map(|scope| scope::matches_current(&lifecycle, &record, &task, scope))
        .transpose()?
        .unwrap_or(true);
    if record.revision != expected_revision
        || !matches!(
            record.status,
            PullRequestPreparationStatus::AwaitingAuthorization
                | PullRequestPreparationStatus::NeedsAttention
        )
        || task.phase != TaskPhase::Done
        || task.pr_work_item_uid.as_deref() != Some(work_item_uid)
        || receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != work_item_uid
        || Some(receipt.subject_sha256.as_str()) != task.implementation_sha256.as_deref()
        || receipt.evidence_refs != [format!("user-action:create-pr:{work_item_uid}")]
        || record
            .publication_authorization
            .as_ref()
            .is_some_and(|existing| existing != receipt)
        || scope.is_some_and(|scope| {
            scope.validate(&record.included_task_uids).is_err()
                || (record.publication_authorization.is_some()
                    && record.publication_scope.is_none())
                || record
                    .publication_scope
                    .as_ref()
                    .is_some_and(|existing| existing != scope)
        })
        || !scope_is_current
        || (record.publication_scope.is_some() && scope.is_none())
    {
        return Err(StoreError::MalformedState(
            "PR publication requires current Done gates and explicit user authorization bound to the reviewed implementation".into(),
        ));
    }
    let already_authorized = record.publication_authorization.is_some();
    let actor = if already_authorized {
        LifecycleActor::ApplicationPolicy
    } else {
        LifecycleActor::User
    };
    record.publication_authorization = Some(receipt.clone());
    if let Some(scope) = scope {
        record.publication_scope = Some(scope.clone());
    }
    record.status = PullRequestPreparationStatus::Publishing;
    if !record
        .publication_authorization_event_uids
        .iter()
        .any(|uid| uid == event_uid)
    {
        record
            .publication_authorization_event_uids
            .push(event_uid.to_owned());
    }
    let recovery = record.recovery.get_or_insert(
        crate::core::lifecycle::store::task_transition::PrPublicationRecoveryV1 {
            attempt: u64::from(already_authorized),
            last_failure: None,
        },
    );
    recovery.attempt = recovery.attempt.checked_add(1).ok_or_else(|| {
        StoreError::MalformedState("PR publication attempt counter exhausted".into())
    })?;
    record.lease_epoch = recovery.attempt;
    before_commit();
    persist(
        store,
        record,
        expected_revision,
        event_uid,
        actor,
        Some(&store_revision),
    )
}
