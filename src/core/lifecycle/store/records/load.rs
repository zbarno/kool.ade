use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::{FeatureGoal, Snapshot, TaskRecord};

pub(in crate::core::lifecycle::store) fn load_features(
    store: &PlanningStore,
) -> Result<Vec<FeatureGoal>, StoreError> {
    store
        .list_record_paths("state/lifecycle_features")?
        .into_iter()
        .map(|path| {
            let (snapshot, revision) = store.read_record::<Snapshot<FeatureGoal>>(&path)?;
            if snapshot.uid != snapshot.record.feature_uid
                || snapshot.revision != revision
                || snapshot.revision != snapshot.record.revision
            {
                return Err(StoreError::MalformedState(
                    "lifecycle feature listing found an inconsistent feature record".into(),
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
