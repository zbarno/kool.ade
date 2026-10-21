use self::validation::{
    allowed_transition, authorization_matches_task, ensure_pr_task_binding, same_pr_identity,
};
use super::super::task_transition::PullRequestPreparationStatus;
use super::super::{
    LifecycleActor, LifecycleEventKind, LifecycleEventRecord, LifecycleStore,
    PullRequestPreparation, Snapshot, TaskEvent, TaskRecord, records::pr_work_item_path,
};
use crate::artifacts::planning_store::StoreError;

mod validation;

pub(super) fn replay_for_task(
    lifecycle: &LifecycleStore<'_>,
    events: &[LifecycleEventRecord],
    task: &TaskRecord,
    tasks: &[TaskRecord],
    restore_snapshot: bool,
) -> Result<(), StoreError> {
    let completions = events.iter().filter(|event| {
        event.task_uid.as_deref() == Some(task.task_uid.as_str())
            && matches!(
                &event.event,
                LifecycleEventKind::Task(TaskEvent::ReconcileSpec { .. })
            )
    });
    let completions = completions.collect::<Vec<_>>();
    let Some(work_item_uid) = task.pr_work_item_uid.as_deref() else {
        if !completions.is_empty() {
            return Err(StoreError::MalformedState(
                "task history created PR work without a task linkage".into(),
            ));
        }
        return Ok(());
    };
    if completions.len() != 1 {
        return Err(StoreError::MalformedState(
            "completed task must have exactly one PR preparation event".into(),
        ));
    }
    let creation = completions[0];
    let LifecycleEventKind::Task(TaskEvent::ReconcileSpec {
        pr_work_item_uid: event_work_item_uid,
        ..
    }) = &creation.event
    else {
        unreachable!("filtered completion events have a reconciliation payload")
    };
    if event_work_item_uid != work_item_uid
        || creation.feature_uid != task.feature_uid
        || creation.actor != LifecycleActor::ApplicationPolicy
        || (creation.schema_version >= 3 && creation.task_revision_after != Some(task.revision))
        || creation.expected_revision.checked_add(1) != Some(task.revision)
    {
        return Err(StoreError::MalformedState(
            "PR preparation event does not match the completed task".into(),
        ));
    }

    let Some(mut preparation) = creation.pr_preparation_after.clone() else {
        // Older v2 histories did not capture the PR projection. They remain
        // readable while the durable record exists, but cannot recover it.
        let existing = lifecycle.load_pr_preparation(work_item_uid)?;
        return ensure_pr_task_binding(
            &existing,
            task,
            work_item_uid,
            events,
            tasks,
            creation.updated_at_ms,
        );
    };
    validate_creation(
        &preparation,
        task,
        work_item_uid,
        events,
        tasks,
        creation.updated_at_ms,
    )?;
    let mut history = vec![(preparation.clone(), preparation.updated_at_ms)];
    let mut transitions = events
        .iter()
        .filter(|event| {
            matches!(
                &event.event,
                LifecycleEventKind::PullRequestState { work_item_uid: event_uid, .. }
                    if event_uid == work_item_uid
            )
        })
        .collect::<Vec<_>>();
    transitions.sort_by_key(|event| event.expected_revision);
    let mut updated_at_ms = preparation.updated_at_ms;
    for event in transitions {
        let LifecycleEventKind::PullRequestState { status, .. } = &event.event else {
            unreachable!("filtered PR events have a status")
        };
        let next = event.pr_preparation_after.as_ref().ok_or_else(|| {
            StoreError::MalformedState("PR event is missing its resulting projection".into())
        })?;
        if event.schema_version < 3
            || event.feature_uid != task.feature_uid
            || event.task_uid.as_deref() != Some(task.task_uid.as_str())
            || event.expected_revision != preparation.revision
            || preparation.revision.checked_add(1) != Some(next.revision)
            || next.status != *status
            || !allowed_transition(preparation.status, next.status)
            || !same_pr_identity(&preparation, next)
            || !validation::valid_authorization_event_uids(&preparation, next, event)
            || (preparation.publication_authorization.is_none()
                && next.publication_authorization.is_some()
                && event.actor != LifecycleActor::User)
            || (!(preparation.publication_authorization.is_none()
                && next.publication_authorization.is_some())
                && event.actor != LifecycleActor::ApplicationPolicy)
            || !validation::valid_recovery_transition(&preparation, next)
        {
            return Err(StoreError::MalformedState(
                "PR event revision does not match its replay position".into(),
            ));
        }
        if preparation.publication_authorization.is_none()
            && next.publication_authorization.is_some()
            && !authorization_matches_task(next, task, work_item_uid)
        {
            return Err(StoreError::MalformedState(
                "PR publication authorization is not bound to this task and user action".into(),
            ));
        }
        next.validate()?;
        preparation = next.clone();
        updated_at_ms = updated_at_ms.max(event.updated_at_ms);
        history.push((preparation.clone(), updated_at_ms));
    }
    ensure_pr_task_binding(
        &preparation,
        task,
        work_item_uid,
        events,
        tasks,
        creation.updated_at_ms,
    )?;
    verify_or_restore(lifecycle, &history, restore_snapshot)
}

fn validate_creation(
    preparation: &PullRequestPreparation,
    task: &TaskRecord,
    work_item_uid: &str,
    events: &[LifecycleEventRecord],
    tasks: &[TaskRecord],
    completed_at_ms: u64,
) -> Result<(), StoreError> {
    preparation.validate()?;
    if preparation.revision != 1
        || preparation.status != PullRequestPreparationStatus::AwaitingAuthorization
        || preparation.publication_authorization.is_some()
        || preparation.provider_pr_id.is_some()
        || preparation.provider_pr_url.is_some()
        || preparation.head_sha.is_some()
        || preparation.base_sha.is_some()
        || preparation.recovery.is_some()
    {
        return Err(StoreError::MalformedState(
            "initial PR preparation state is invalid".into(),
        ));
    }
    ensure_pr_task_binding(
        preparation,
        task,
        work_item_uid,
        events,
        tasks,
        completed_at_ms,
    )
}

fn verify_or_restore(
    lifecycle: &LifecycleStore<'_>,
    history: &[(PullRequestPreparation, u64)],
    restore_snapshot: bool,
) -> Result<(), StoreError> {
    let (initial, initial_updated_at_ms) = history
        .first()
        .ok_or_else(|| StoreError::MalformedState("PR history is empty".into()))?;
    let current = match lifecycle.load_pr_preparation(&initial.work_item_uid) {
        Ok(snapshot) => snapshot,
        Err(StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound && !restore_snapshot =>
        {
            return Ok(());
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            let snapshot = Snapshot {
                uid: initial.work_item_uid.clone(),
                schema_version: 1,
                created_at_ms: initial.created_at_ms,
                updated_at_ms: *initial_updated_at_ms,
                revision: initial.revision,
                record: initial.clone(),
            };
            let path = pr_work_item_path(&initial.work_item_uid)?;
            match lifecycle.store.save_record(&path, &snapshot, 0) {
                Ok(_) => initial.clone(),
                Err(StoreError::StaleRecordRevision { .. }) => lifecycle
                    .load_pr_preparation(&initial.work_item_uid)
                    .map_err(|_| {
                        StoreError::MalformedState(
                            "concurrent PR recovery produced no readable snapshot".into(),
                        )
                    })?,
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let mut index = history
        .iter()
        .position(|(state, _)| *state == current)
        .ok_or_else(|| {
            StoreError::MalformedState(
                "replayed PR preparation does not match its durable snapshot".into(),
            )
        })?;
    if !restore_snapshot {
        return Ok(());
    }
    while index + 1 < history.len() {
        let (preparation, updated_at_ms) = &history[index + 1];
        let snapshot = Snapshot {
            uid: preparation.work_item_uid.clone(),
            schema_version: 1,
            created_at_ms: preparation.created_at_ms,
            updated_at_ms: *updated_at_ms,
            revision: preparation.revision,
            record: preparation.clone(),
        };
        let path = pr_work_item_path(&preparation.work_item_uid)?;
        match lifecycle
            .store
            .save_record(&path, &snapshot, history[index].0.revision)
        {
            Ok(_) => index += 1,
            Err(StoreError::StaleRecordRevision { .. }) => {
                let concurrent = lifecycle.load_pr_preparation(&preparation.work_item_uid)?;
                let Some(concurrent_index) =
                    history.iter().position(|(state, _)| *state == concurrent)
                else {
                    return Err(StoreError::MalformedState(
                        "concurrent PR recovery produced a conflicting snapshot".into(),
                    ));
                };
                if concurrent_index <= index {
                    return Err(StoreError::MalformedState(
                        "concurrent PR recovery did not advance the snapshot".into(),
                    ));
                }
                index = concurrent_index;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
