use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::{PullRequestPreparation, Snapshot};

use super::pr_work_item_path;

pub(in crate::core::lifecycle::store) fn load(
    store: &PlanningStore,
    work_item_uid: &str,
) -> Result<PullRequestPreparation, StoreError> {
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
