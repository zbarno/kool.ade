use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::lifecycle::{GateActor, GateDecision, GateReceipt, TaskPhase},
};

use super::{
    LifecycleActor, LifecycleEventKind, LifecycleStore, Snapshot,
    records::{append_transition, event_path, event_record, now_unix_ms, pr_work_item_path},
    task_transition::{
        PullRequestPreparation, PullRequestPreparationStatus, PullRequestPublicationState,
    },
};

pub(super) fn authorize(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    receipt: &GateReceipt,
) -> Result<PullRequestPreparation, StoreError> {
    receipt.validate().map_err(StoreError::MalformedState)?;
    let mut record = read(store, work_item_uid)?;
    if record.publication_authorization.as_ref() == Some(receipt)
        && record.status != PullRequestPreparationStatus::AwaitingAuthorization
    {
        return Ok(record);
    }
    let task = LifecycleStore::new(store).load_task(&record.task_uid)?;
    let lifecycle = LifecycleStore::new(store);
    let pending_triggers = lifecycle
        .list_spec_sync_queue(&record.feature_uid, false)?
        .into_iter()
        .filter(|entry| {
            entry.source_task_uid.as_deref().is_none_or(|task_uid| {
                record
                    .included_task_uids
                    .iter()
                    .any(|included| included == task_uid)
            })
        })
        .collect::<Vec<_>>();
    if !pending_triggers.is_empty() {
        return Err(StoreError::MalformedState(format!(
            "PR authorization is blocked by {} pending specification trigger(s) for this feature and task scope",
            pending_triggers.len()
        )));
    }
    if record.revision != expected_revision
        || record.status != PullRequestPreparationStatus::AwaitingAuthorization
        || task.phase != TaskPhase::Done
        || task.pr_work_item_uid.as_deref() != Some(work_item_uid)
        || receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != work_item_uid
        || Some(receipt.subject_sha256.as_str()) != task.implementation_sha256.as_deref()
        || receipt.evidence_refs != [format!("user-action:create-pr:{work_item_uid}")]
    {
        return Err(StoreError::MalformedState(
            "PR publication requires current Done gates and explicit user authorization bound to the reviewed implementation".into(),
        ));
    }
    record.publication_authorization = Some(receipt.clone());
    record.status = PullRequestPreparationStatus::Publishing;
    persist(
        store,
        record,
        expected_revision,
        event_uid,
        LifecycleActor::User,
    )
}

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
    if !allowed || record.revision != expected_revision {
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
    record.status = status;
    record.provider_pr_id = provider_pr_id;
    record.provider_pr_url = provider_pr_url;
    record.head_sha = head_sha;
    record.base_sha = base_sha;
    record.merged_commit_sha = merged_commit_sha;
    let actor = LifecycleActor::ApplicationPolicy;
    persist(store, record, expected_revision, event_uid, actor)
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
    append_transition(
        store,
        &path,
        &snapshot,
        expected_revision,
        event_path(event_uid)?,
        event,
    )?;
    Ok(record)
}
