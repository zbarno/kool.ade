use super::super::super::{LifecycleActor, LifecycleEventKind, LifecycleEventRecord, TaskRecord};
use crate::artifacts::planning_store::StoreError;

pub(super) fn validate_parent_subtasks(
    events: &[LifecycleEventRecord],
    parent: &TaskRecord,
) -> Result<(), StoreError> {
    let children = events
        .iter()
        .filter_map(|event| {
            let LifecycleEventKind::SubtaskCreated { parent_task_uid } = &event.event else {
                return None;
            };
            (parent_task_uid == &parent.task_uid)
                .then_some(event.initial_task.as_ref())
                .flatten()
        })
        .map(|task| task.task_uid.as_str())
        .collect::<Vec<_>>();
    if children.len() != parent.subtask_uids.len()
        || parent.subtask_uids.iter().any(|uid| {
            children
                .iter()
                .filter(|child_uid| **child_uid == uid)
                .count()
                != 1
        })
    {
        return Err(StoreError::MalformedState(
            "parent task subtask list does not match its creation events".into(),
        ));
    }
    for event in events.iter().filter(|event| {
        event.schema_version >= 3
            && event.parent_task_after.is_none()
            && matches!(
                &event.event,
                LifecycleEventKind::SubtaskCreated { parent_task_uid }
                    if parent_task_uid == &parent.task_uid
            )
    }) {
        let child_uid = event
            .initial_task
            .as_ref()
            .map(|task| task.task_uid.as_str())
            .ok_or_else(|| {
                StoreError::MalformedState("subtask event has no initial child record".into())
            })?;
        if !parent.subtask_uids.iter().any(|uid| uid == child_uid) {
            return Err(StoreError::MalformedState(
                "subtask event without a parent projection is absent from the parent graph".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn replay_parent_subtask_creation(
    record: &LifecycleEventRecord,
    feature_uid: &str,
    parent_uid: &str,
    child_parent_uid: &str,
    parent: &mut TaskRecord,
    next_revision: u64,
) -> Result<(), StoreError> {
    let child = record.initial_task.as_ref().ok_or_else(|| {
        StoreError::MalformedState("subtask event has no initial child record".into())
    })?;
    let parent_after = record.parent_task_after.as_ref().ok_or_else(|| {
        StoreError::MalformedState("subtask event has no parent projection".into())
    })?;
    if record.schema_version < 3
        || record.actor != LifecycleActor::ApplicationPolicy
        || record.feature_uid != feature_uid
        || record.task_uid.as_deref() != Some(child.task_uid.as_str())
        || record.expected_revision != 0
        || record.task_revision_after != Some(1)
        || child.feature_uid != feature_uid
        || child.parent_task_uid.as_deref() != Some(parent_uid)
        || child.revision != 0
        || child_parent_uid != parent_uid
        || parent.revision + 1 != next_revision
        || parent_after.task_uid != parent_uid
        || parent_after.feature_uid != feature_uid
        || parent_after.revision != next_revision
        || parent.subtask_uids.contains(&child.task_uid)
    {
        return Err(StoreError::MalformedState(
            "subtask event does not match its parent task revision".into(),
        ));
    }
    let mut expected = parent.clone();
    expected.subtask_uids.push(child.task_uid.clone());
    expected.revision = next_revision;
    expected.validate().map_err(StoreError::MalformedState)?;
    parent_after
        .validate()
        .map_err(StoreError::MalformedState)?;
    if *parent_after != expected {
        return Err(StoreError::MalformedState(
            "subtask event parent projection is inconsistent".into(),
        ));
    }
    *parent = parent_after.clone();
    Ok(())
}
