use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::{FeatureEvent, FeatureGoal};

use super::{
    Snapshot,
    records::{
        EventProjection, append_transition, event_path, event_record, feature_actor, feature_path,
        now_unix_ms, stale,
    },
    task_transition,
};

#[cfg(test)]
#[path = "feature_event/tests.rs"]
mod tests;

pub(super) fn apply(
    store: &PlanningStore,
    feature_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: FeatureEvent,
) -> Result<FeatureGoal, StoreError> {
    let (path, snapshot) = read_for_apply(store, feature_uid, expected_revision)?;
    commit_after_read(
        store,
        feature_uid,
        &path,
        expected_revision,
        event_uid,
        proposal,
        snapshot,
    )
}

fn read_for_apply(
    store: &PlanningStore,
    feature_uid: &str,
    expected_revision: u64,
) -> Result<(String, Snapshot<FeatureGoal>), StoreError> {
    let path = feature_path(feature_uid);
    let (snapshot, actual) = store.read_record::<Snapshot<FeatureGoal>>(&path)?;
    if actual != expected_revision || snapshot.record.revision != expected_revision {
        return Err(stale(&path, expected_revision, actual));
    }
    Ok((path, snapshot))
}

fn commit_after_read(
    store: &PlanningStore,
    feature_uid: &str,
    path: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: FeatureEvent,
    mut snapshot: Snapshot<FeatureGoal>,
) -> Result<FeatureGoal, StoreError> {
    if matches!(proposal, FeatureEvent::TasksCompleted) {
        task_transition::ensure_feature_tasks_complete(
            store,
            feature_uid,
            &snapshot.record.task_uids,
        )?;
    }
    snapshot
        .record
        .apply(expected_revision, proposal.clone())
        .map_err(StoreError::MalformedState)?;
    snapshot.revision = snapshot.record.revision;
    snapshot.updated_at_ms = now_unix_ms();
    let event_path = event_path(event_uid)?;
    let event = event_record(
        event_uid,
        feature_uid,
        None,
        expected_revision,
        feature_actor(&proposal),
        super::LifecycleEventKind::Feature(proposal),
        EventProjection {
            feature_revision_after: Some(snapshot.revision),
            ..EventProjection::default()
        },
    )?;
    append_transition(store, path, &snapshot, expected_revision, event_path, event)?;
    Ok(snapshot.record)
}
