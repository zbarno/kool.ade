use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::{Snapshot, TaskRecord};

pub(in crate::core::lifecycle::store) fn load_tasks(
    store: &PlanningStore,
) -> Result<Vec<TaskRecord>, StoreError> {
    store
        .list_record_paths("state/lifecycle_tasks")?
        .into_iter()
        .map(|path| {
            let (snapshot, revision) = store.read_record::<Snapshot<TaskRecord>>(&path)?;
            if snapshot.uid != snapshot.record.task_uid
                || snapshot.revision != revision
                || snapshot.revision != snapshot.record.revision
            {
                return Err(StoreError::MalformedState(
                    "lifecycle task listing found an inconsistent task record".into(),
                ));
            }
            snapshot
                .record
                .validate()
                .map_err(StoreError::MalformedState)?;
            Ok(snapshot.record)
        })
        .collect()
}
