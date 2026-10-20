use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        LifecycleActor, LifecycleEventKind, LifecycleStore, PullRequestPreparation,
        PullRequestPreparationStatus, SpecSyncTriggerKindV1,
    },
};

pub(super) fn check(
    lifecycle: &LifecycleStore<'_>,
    work: &PullRequestPreparation,
) -> Result<(), StoreError> {
    lifecycle.ensure_product_specification_current(&work.feature_uid)?;
    let authorization_events = lifecycle
        .events_for_feature(&work.feature_uid)?
        .into_iter()
        .filter(|event| {
            event.actor == LifecycleActor::User
                && event.task_uid.as_deref() == Some(work.task_uid.as_str())
                && matches!(&event.event, LifecycleEventKind::PullRequestState { work_item_uid, status }
                    if work_item_uid == &work.work_item_uid && *status == PullRequestPreparationStatus::Publishing)
                && event.pr_preparation_after.as_ref().is_some_and(|after| {
                    after.revision == event.expected_revision.saturating_add(1)
                        && after.work_item_uid == work.work_item_uid
                        && after.feature_uid == work.feature_uid
                        && after.task_uid == work.task_uid
                        && after.included_task_uids == work.included_task_uids
                        && after.status == PullRequestPreparationStatus::Publishing
                        && after.publication_authorization.is_some()
                        && after.publication_authorization == work.publication_authorization
                })
        })
        .map(|event| event.uid)
        .collect::<Vec<_>>();
    let exempt = (matches!(
        work.status,
        PullRequestPreparationStatus::Publishing | PullRequestPreparationStatus::NeedsAttention
    ) && authorization_events.len() == 1)
        .then(|| authorization_events[0].as_str());
    let pending = lifecycle.list_spec_sync_queue(&work.feature_uid, false)?;
    let blocked = pending.iter().any(|entry| {
        let in_scope = entry
            .source_task_uid
            .as_deref()
            .is_none_or(|task_uid| work.included_task_uids.iter().any(|uid| uid == task_uid));
        let exact_authorization = exempt == Some(entry.uid.as_str())
            && entry.source_event_uid == entry.uid
            && entry.feature_uid == work.feature_uid
            && entry.source_task_uid.as_deref() == Some(work.task_uid.as_str())
            && entry.trigger == SpecSyncTriggerKindV1::PullRequest;
        in_scope && !exact_authorization
    });
    if blocked {
        return Err(StoreError::MalformedState(
            "PR authorization is blocked by pending specification trigger(s) for this feature and task scope".into(),
        ));
    }
    Ok(())
}
