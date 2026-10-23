use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskPhase},
};

use super::{
    LifecycleActor, LifecycleEventKind, LifecycleStore, Snapshot,
    records::{
        append_transition_at_store_revision, event_path, event_record, now_unix_ms,
        pr_work_item_path,
    },
    task_transition::{
        PullRequestPreparation, PullRequestPreparationStatus, PullRequestPublicationState,
    },
};

mod authorize;
mod gate;
mod recovery;
mod scope;
pub(super) use authorize::authorize;
#[cfg(test)]
pub(in crate::core::lifecycle) use authorize::authorize_before_commit;

#[allow(clippy::too_many_arguments)]
pub(super) fn record_state(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    update: PullRequestPublicationState,
) -> Result<PullRequestPreparation, StoreError> {
    let PullRequestPublicationState {
        status,
        provider_pr_id,
        provider_pr_url,
        head_sha,
        base_sha,
        merged_commit_sha,
        lease_epoch,
    } = update;
    let mut record = read(store, work_item_uid)?;
    if record.status == status
        && record.provider_pr_id == provider_pr_id
        && record.provider_pr_url == provider_pr_url
        && record.head_sha == head_sha
        && record.base_sha == base_sha
        && record.merged_commit_sha == merged_commit_sha
    {
        return Ok(record);
    }
    let allowed = matches!(
        (record.status, status),
        (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::Opened
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::ClosedUnmerged
        ) | (
            PullRequestPreparationStatus::Publishing,
            PullRequestPreparationStatus::NeedsAttention,
        ) | (
            PullRequestPreparationStatus::NeedsAttention,
            PullRequestPreparationStatus::Publishing,
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::Opened
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::ClosedUnmerged
        ) | (
            PullRequestPreparationStatus::Opened,
            PullRequestPreparationStatus::Merged
        ) | (
            PullRequestPreparationStatus::ClosedUnmerged,
            PullRequestPreparationStatus::Opened
        )
    );
    if !allowed
        || record.revision != expected_revision
        || ((record.status == PullRequestPreparationStatus::Publishing || lease_epoch.is_some())
            && lease_epoch != Some(record.publication_lease_epoch()))
    {
        return Err(StoreError::MalformedState(
            "pull request status transition is stale or not allowed".into(),
        ));
    }
    if status == PullRequestPreparationStatus::Merged
        && merged_commit_sha
            .as_deref()
            .is_none_or(|sha| !super::super::types::is_git_oid(sha))
    {
        return Err(StoreError::MalformedState(
            "merged pull request requires an authoritative merge commit".into(),
        ));
    }
    if status == PullRequestPreparationStatus::Publishing {
        let prior_epoch = record.publication_lease_epoch();
        let recovery = record
            .recovery
            .get_or_insert(super::PrPublicationRecoveryV1 {
                attempt: prior_epoch,
                last_failure: None,
            });
        recovery.attempt = prior_epoch.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("PR publication attempt counter exhausted".into())
        })?;
        record.lease_epoch = recovery.attempt;
    }
    record.status = status;
    record.provider_pr_id = provider_pr_id;
    record.provider_pr_url = provider_pr_url;
    record.head_sha = head_sha;
    record.base_sha = base_sha;
    record.merged_commit_sha = merged_commit_sha;
    let actor = LifecycleActor::ApplicationPolicy;
    persist(store, record, expected_revision, event_uid, actor, None)
}

fn read(store: &PlanningStore, work_item_uid: &str) -> Result<PullRequestPreparation, StoreError> {
    let path = pr_work_item_path(work_item_uid)?;
    let (snapshot, actual) = store.read_record::<Snapshot<PullRequestPreparation>>(&path)?;
    let record = snapshot.record;
    if snapshot.uid != work_item_uid
        || snapshot.revision != actual
        || snapshot.revision != record.revision
    {
        return Err(StoreError::MalformedState(
            "pull request preparation snapshot revision is inconsistent".into(),
        ));
    }
    record.validate()?;
    if record.work_item_uid != work_item_uid {
        return Err(StoreError::MalformedState(
            "pull request preparation identity mismatch".into(),
        ));
    }
    Ok(record)
}

fn persist(
    store: &PlanningStore,
    mut record: PullRequestPreparation,
    expected_revision: u64,
    event_uid: &str,
    actor: LifecycleActor,
    store_revision: Option<&str>,
) -> Result<PullRequestPreparation, StoreError> {
    let path = pr_work_item_path(&record.work_item_uid)?;
    let (mut snapshot, actual) = store.read_record::<Snapshot<PullRequestPreparation>>(&path)?;
    if actual != expected_revision
        || snapshot.revision != expected_revision
        || snapshot.record != read(store, &record.work_item_uid)?
    {
        return Err(StoreError::MalformedState(
            "pull request preparation revision is stale".into(),
        ));
    }
    record.revision = expected_revision + 1;
    record.updated_at_ms = now_unix_ms();
    record.validate()?;
    snapshot.revision = record.revision;
    snapshot.updated_at_ms = record.updated_at_ms;
    snapshot.record = record.clone();
    let event = event_record(
        event_uid,
        &record.feature_uid,
        Some(&record.task_uid),
        expected_revision,
        actor,
        LifecycleEventKind::PullRequestState {
            work_item_uid: record.work_item_uid.clone(),
            status: record.status,
        },
        super::records::EventProjection {
            pr_preparation_after: Some(record.clone()),
            ..super::records::EventProjection::default()
        },
    )?;
    append_transition_at_store_revision(
        store,
        &path,
        &snapshot,
        expected_revision,
        event_path(event_uid)?,
        event,
        store_revision,
    )?;
    Ok(record)
}
