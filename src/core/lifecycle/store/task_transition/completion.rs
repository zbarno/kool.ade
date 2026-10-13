use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::lifecycle::{TaskPhase, TaskRecord},
};

use super::super::{Snapshot, records::task_path};
use super::load;

pub(in crate::core::lifecycle::store) fn ensure_feature_tasks_complete(
    store: &PlanningStore,
    feature_uid: &str,
    task_uids: &[String],
) -> Result<(), StoreError> {
    if task_uids.is_empty() {
        return Err(StoreError::MalformedState(
            "feature cannot complete without a task graph".into(),
        ));
    }
    for task_uid in task_uids {
        let path = task_path(task_uid);
        let (snapshot, actual) = store.read_record::<Snapshot<TaskRecord>>(&path)?;
        let task = snapshot.record;
        if snapshot.uid != *task_uid
            || task.task_uid != *task_uid
            || task.feature_uid != feature_uid
            || snapshot.revision != actual
            || snapshot.revision != task.revision
        {
            return Err(StoreError::MalformedState(
                "feature task graph contains an inconsistent task record".into(),
            ));
        }
        task.validate().map_err(StoreError::MalformedState)?;
        if !matches!(
            task.phase,
            TaskPhase::Done | TaskPhase::PrPending | TaskPhase::PrOpened | TaskPhase::PrMerged
        ) {
            return Err(StoreError::MalformedState(
                "feature cannot complete while a task is unfinished".into(),
            ));
        }
        if let Some(parent_uid) = task.parent_task_uid.as_deref() {
            let (parent, parent_actual) =
                store.read_record::<Snapshot<TaskRecord>>(&task_path(parent_uid))?;
            if parent.uid != parent_uid
                || parent.revision != parent_actual
                || parent.record.revision != parent_actual
                || parent.record.task_uid != parent_uid
                || !parent.record.subtask_uids.contains(task_uid)
                || parent.record.feature_uid != feature_uid
                || parent.record.parent_task_uid.is_some()
                || !matches!(
                    parent.record.phase,
                    TaskPhase::Done
                        | TaskPhase::PrPending
                        | TaskPhase::PrOpened
                        | TaskPhase::PrMerged
                )
            {
                return Err(StoreError::MalformedState(
                    "completed subtask has no completed standalone parent".into(),
                ));
            }
            let parent_work_uid = parent.record.pr_work_item_uid.as_deref().ok_or_else(|| {
                StoreError::MalformedState("standalone parent has no PR preparation item".into())
            })?;
            let parent_work = load(store, parent_work_uid)?;
            if !parent_work.included_task_uids.contains(task_uid) {
                return Err(StoreError::MalformedState(
                    "parent PR preparation does not include its completed subtask".into(),
                ));
            }
            continue;
        }
        let work_uid = task.pr_work_item_uid.as_deref().ok_or_else(|| {
            StoreError::MalformedState("completed task has no PR preparation work item".into())
        })?;
        let work = load(store, work_uid)?;
        if work.feature_uid != feature_uid || work.task_uid != task_uid.as_str() {
            return Err(StoreError::MalformedState(
                "PR preparation work item does not match its completed task".into(),
            ));
        }
    }
    Ok(())
}
