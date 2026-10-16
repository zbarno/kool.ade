use crate::artifacts::planning_store::{RecordRevisionCheck, StoreError};
use crate::core::lifecycle::SpecRevisionV1;

use super::super::super::{
    LifecycleStore, Snapshot, TaskRecord, records::task_path,
    task_transition::PullRequestPreparation,
};

pub(super) struct CurrentImplementationSources {
    pub task: TaskRecord,
    pub work: PullRequestPreparation,
    pub specification: SpecRevisionV1,
    pub checks: [RecordRevisionCheck; 3],
}

pub(super) fn load(
    lifecycle: &LifecycleStore<'_>,
    task_uid: &str,
) -> Result<CurrentImplementationSources, StoreError> {
    let task = lifecycle.load_task(task_uid)?;
    let task_path = task_path(task_uid);
    let (task_snapshot, task_record_revision) = lifecycle
        .store
        .read_record::<Snapshot<TaskRecord>>(&task_path)?;
    if task_snapshot.record != task || task_snapshot.revision != task_record_revision {
        return Err(StoreError::StaleRecordRevision {
            path: task_path,
            expected: task.revision,
            actual: task_record_revision,
        });
    }

    let work_uid = task.pr_work_item_uid.as_deref().ok_or_else(|| {
        StoreError::MalformedState("current implementation requires a linked PR record".into())
    })?;
    let work = lifecycle.load_pr_preparation(work_uid)?;
    let work_path = super::super::super::records::pr_work_item_path(work_uid)?;
    let (work_snapshot, work_record_revision) = lifecycle
        .store
        .read_record::<Snapshot<PullRequestPreparation>>(&work_path)?;
    if work_snapshot.record != work || work_snapshot.revision != work_record_revision {
        return Err(StoreError::StaleRecordRevision {
            path: work_path,
            expected: work.revision,
            actual: work_record_revision,
        });
    }

    let specification = lifecycle
        .load_spec_revision(&task.feature_uid)?
        .ok_or_else(|| {
            StoreError::MalformedState(
                "current implementation requires a recorded specification baseline".into(),
            )
        })?;
    let specification_path = format!("state/spec_revisions/{}.json", task.feature_uid);
    let (persisted_specification, specification_record_revision) = lifecycle
        .store
        .read_record::<SpecRevisionV1>(&specification_path)?;
    if persisted_specification != specification
        || specification.revision != specification_record_revision
    {
        return Err(StoreError::StaleRecordRevision {
            path: specification_path,
            expected: specification.revision,
            actual: specification_record_revision,
        });
    }

    Ok(CurrentImplementationSources {
        task,
        work,
        specification,
        checks: [
            RecordRevisionCheck {
                path: task_path,
                expected_revision: task_record_revision,
            },
            RecordRevisionCheck {
                path: work_path,
                expected_revision: work_record_revision,
            },
            RecordRevisionCheck {
                path: specification_path,
                expected_revision: specification_record_revision,
            },
        ],
    })
}
