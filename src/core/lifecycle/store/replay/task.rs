use self::subtask::{replay_parent_subtask_creation, validate_parent_subtasks};
use super::super::{
    LifecycleActor, LifecycleEventKind, LifecycleEventRecord, LifecycleStore, Snapshot, TaskEvent,
    TaskRecord,
    records::{task_actor, task_path},
};
use crate::artifacts::planning_store::StoreError;

mod subtask;

pub(super) fn replay_task(
    lifecycle: &LifecycleStore<'_>,
    events: &[LifecycleEventRecord],
    feature_uid: &str,
    task_uid: &str,
    restore_snapshot: bool,
) -> Result<TaskRecord, StoreError> {
    let mut creations = events.iter().filter(|event| {
        event.task_uid.as_deref() == Some(task_uid)
            && matches!(
                event.event,
                LifecycleEventKind::TaskCreated | LifecycleEventKind::SubtaskCreated { .. }
            )
    });
    let creation = creations.next().ok_or_else(|| {
        StoreError::MalformedState("task event history has no creation event".into())
    })?;
    if creations.next().is_some()
        || creation.schema_version < 2
        || creation.feature_uid != feature_uid
        || creation.expected_revision != 0
        || creation.actor != LifecycleActor::ApplicationPolicy
        || creation.initial_feature.is_some()
        || creation.initial_task.is_none()
        || creation.pr_preparation_after.is_some()
    {
        return Err(StoreError::MalformedState(
            "task creation event is not replayable".into(),
        ));
    }
    let mut task = creation.initial_task.clone().ok_or_else(|| {
        StoreError::MalformedState("task history has no initial task record".into())
    })?;
    if task.task_uid != task_uid
        || task.feature_uid != feature_uid
        || task.revision != 0
        || task.phase != crate::core::lifecycle::TaskPhase::Ready
    {
        return Err(StoreError::MalformedState(
            "task creation event has an invalid initial task".into(),
        ));
    }
    if creation.schema_version >= 3 && creation.task_revision_after != Some(1) {
        return Err(StoreError::MalformedState(
            "task creation event is missing its resulting task revision".into(),
        ));
    }
    task.revision = 1;
    task.validate().map_err(StoreError::MalformedState)?;
    let mut updated_at_ms = creation.updated_at_ms;
    let mut history = vec![(task.clone(), updated_at_ms)];

    let mut transitions = events
        .iter()
        .filter_map(|event| match &event.event {
            LifecycleEventKind::Task(_) if event.task_uid.as_deref() == Some(task_uid) => {
                Some((event.expected_revision, event))
            }
            LifecycleEventKind::SubtaskCreated { parent_task_uid }
                if parent_task_uid == task_uid && event.parent_task_after.is_some() =>
            {
                event
                    .parent_task_after
                    .as_ref()
                    .and_then(|parent| parent.revision.checked_sub(1))
                    .map(|expected| (expected, event))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    transitions.sort_by_key(|(revision, _)| *revision);
    for (expected_revision, record) in transitions {
        let next_revision = task
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("task revision is exhausted".into()))?;
        match &record.event {
            LifecycleEventKind::Task(proposal) => {
                if record.schema_version < 2
                    || record.feature_uid != feature_uid
                    || record.expected_revision != task.revision
                    || record.feature_revision_after.is_some()
                    || record.initial_feature.is_some()
                    || record.initial_task.is_some()
                    || record.parent_task_after.is_some()
                    || record.actor != task_actor(proposal)
                    || (record.schema_version >= 3
                        && record.task_revision_after != Some(next_revision))
                    || (record.schema_version < 3 && record.task_revision_after.is_some())
                {
                    return Err(StoreError::MalformedState(format!(
                        "task event {} has inconsistent replay metadata (expected {}, current {}, resulting {:?})",
                        record.uid,
                        record.expected_revision,
                        task.revision,
                        record.task_revision_after,
                    )));
                }
                if matches!(proposal, TaskEvent::ReconcileSpec { .. })
                    != record.pr_preparation_after.is_some()
                    && record.schema_version >= 3
                {
                    return Err(StoreError::MalformedState(
                        "task completion event is missing its PR preparation projection".into(),
                    ));
                }
                if !matches!(proposal, TaskEvent::ReconcileSpec { .. })
                    && record.pr_preparation_after.is_some()
                {
                    return Err(StoreError::MalformedState(
                        "non-completion task event contains a PR preparation projection".into(),
                    ));
                }
                task.apply(task.revision, proposal.clone())
                    .map_err(StoreError::MalformedState)?;
            }
            LifecycleEventKind::SubtaskCreated { parent_task_uid } => {
                replay_parent_subtask_creation(
                    record,
                    feature_uid,
                    task_uid,
                    parent_task_uid,
                    &mut task,
                    next_revision,
                )?;
            }
            _ => unreachable!("filtered task projections contain only supported events"),
        }
        if task.revision != next_revision || expected_revision != task.revision - 1 {
            return Err(StoreError::MalformedState(
                "task reducer produced an unexpected revision".into(),
            ));
        }
        updated_at_ms = updated_at_ms.max(record.updated_at_ms);
        history.push((task.clone(), updated_at_ms));
    }

    validate_parent_subtasks(events, &task)?;

    verify_or_restore_task(
        lifecycle,
        &history,
        creation.created_at_ms,
        restore_snapshot,
    )?;
    Ok(task)
}

fn verify_or_restore_task(
    lifecycle: &LifecycleStore<'_>,
    history: &[(TaskRecord, u64)],
    created_at_ms: u64,
    restore_snapshot: bool,
) -> Result<(), StoreError> {
    let (initial, initial_updated_at_ms) = history
        .first()
        .ok_or_else(|| StoreError::MalformedState("task history is empty".into()))?;
    let current = match lifecycle.load_task(&initial.task_uid) {
        Ok(snapshot) => snapshot,
        Err(StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound && !restore_snapshot =>
        {
            return Ok(());
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            let snapshot = Snapshot {
                uid: initial.task_uid.clone(),
                schema_version: 1,
                created_at_ms,
                updated_at_ms: *initial_updated_at_ms,
                revision: initial.revision,
                record: initial.clone(),
            };
            let path = task_path(&initial.task_uid);
            match lifecycle.store.save_record(&path, &snapshot, 0) {
                Ok(_) => initial.clone(),
                Err(StoreError::StaleRecordRevision { .. }) => {
                    lifecycle.load_task(&initial.task_uid).map_err(|_| {
                        StoreError::MalformedState(
                            "concurrent task recovery produced no readable snapshot".into(),
                        )
                    })?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let mut index = history
        .iter()
        .position(|(state, _)| *state == current)
        .ok_or_else(|| {
            StoreError::MalformedState("replayed task does not match its durable snapshot".into())
        })?;
    if !restore_snapshot {
        return Ok(());
    }
    while index + 1 < history.len() {
        let (task, updated_at_ms) = &history[index + 1];
        let snapshot = Snapshot {
            uid: task.task_uid.clone(),
            schema_version: 1,
            created_at_ms,
            updated_at_ms: *updated_at_ms,
            revision: task.revision,
            record: task.clone(),
        };
        let path = task_path(&task.task_uid);
        match lifecycle
            .store
            .save_record(&path, &snapshot, history[index].0.revision)
        {
            Ok(_) => index += 1,
            Err(StoreError::StaleRecordRevision { .. }) => {
                let concurrent = lifecycle.load_task(&task.task_uid)?;
                let Some(concurrent_index) =
                    history.iter().position(|(state, _)| *state == concurrent)
                else {
                    return Err(StoreError::MalformedState(
                        "concurrent task recovery produced a conflicting snapshot".into(),
                    ));
                };
                if concurrent_index <= index {
                    return Err(StoreError::MalformedState(
                        "concurrent task recovery did not advance the snapshot".into(),
                    ));
                }
                index = concurrent_index;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
