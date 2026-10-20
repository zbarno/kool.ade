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

mod gate;
mod recovery;

pub(super) fn authorize(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    receipt: &GateReceipt,
) -> Result<PullRequestPreparation, StoreError> {
    authorize_before_commit(
        store,
        work_item_uid,
        expected_revision,
        event_uid,
        receipt,
        || {},
    )
}

pub(in crate::core::lifecycle) fn authorize_before_commit(
    store: &PlanningStore,
    work_item_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    receipt: &GateReceipt,
    before_commit: impl FnOnce(),
) -> Result<PullRequestPreparation, StoreError> {
    let store_revision = store.revision()?;
    receipt.validate().map_err(StoreError::MalformedState)?;
    let mut record = read(store, work_item_uid)?;
    let lifecycle = LifecycleStore::new(store);
    gate::check(&lifecycle, &record)?;
    if record.publication_authorization.as_ref() == Some(receipt)
        && matches!(
            record.status,
            PullRequestPreparationStatus::Publishing | PullRequestPreparationStatus::Opened
        )
    {
        before_commit();
        // The locked revision check is the retry attempt's linearization point.
        store.transaction_with_revision_and_record_revisions(&[], Some(&store_revision), &[])?;
        return Ok(record);
    }
    let task = lifecycle.load_task(&record.task_uid)?;
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
    record.status = PullRequestPreparationStatus::Publishing;
    let recovery = record
        .recovery
        .get_or_insert(super::PrPublicationRecoveryV1 {
            attempt: u64::from(already_authorized),
            last_failure: None,
        });
    recovery.attempt = recovery.attempt.checked_add(1).ok_or_else(|| {
        StoreError::MalformedState("PR publication attempt counter exhausted".into())
    })?;
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
    if status == PullRequestPreparationStatus::Publishing
        && let Some(recovery) = record.recovery.as_mut()
    {
        recovery.attempt = recovery.attempt.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("PR publication attempt counter exhausted".into())
        })?;
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
