use super::super::{LifecycleEventKind, LifecycleEventRecord, TaskRecord};
use crate::artifacts::planning_store::StoreError;

pub(super) fn validate_events(
    events: &[LifecycleEventRecord],
    tasks: &[TaskRecord],
) -> Result<(), StoreError> {
    for event in events {
        match &event.event {
            LifecycleEventKind::Task(_) => {
                let task_uid = event.task_uid.as_deref().ok_or_else(|| {
                    StoreError::MalformedState("task event has no owning task identity".into())
                })?;
                if !tasks.iter().any(|task| task.task_uid == task_uid) {
                    return Err(StoreError::MalformedState(
                        "task event is not owned by the feature task graph".into(),
                    ));
                }
            }
            LifecycleEventKind::PullRequestState { work_item_uid, .. } => {
                let task_uid = event.task_uid.as_deref().ok_or_else(|| {
                    StoreError::MalformedState("PR event has no owning task identity".into())
                })?;
                if !tasks.iter().any(|task| {
                    task.task_uid == task_uid
                        && task.pr_work_item_uid.as_deref() == Some(work_item_uid.as_str())
                }) {
                    return Err(StoreError::MalformedState(
                        "PR event is not owned by a task preparation in the feature graph".into(),
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
