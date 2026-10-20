use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::TaskPhase;

use super::{
    FeatureGoal, LifecycleEventKind, Snapshot, TaskEvent, TaskRecord,
    records::{
        append_transition_with_additional_checks, encode_record, event_path, event_record,
        feature_path, now_unix_ms, pr_work_item_path, stale, task_actor, task_path,
    },
};

mod completion;
mod load;
pub(super) use completion::ensure_feature_tasks_complete;
pub(super) use load::load;

mod publication;
pub use publication::{
    PrPublicationRecoveryV1, PullRequestPreparation, PullRequestPreparationStatus,
    PullRequestPublicationState,
};

pub(super) fn apply_task_event(
    store: &PlanningStore,
    task_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: TaskEvent,
) -> Result<TaskRecord, StoreError> {
    let path = task_path(task_uid);
    let (mut snapshot, actual) = store.read_record::<Snapshot<TaskRecord>>(&path)?;
    if actual != expected_revision || snapshot.record.revision != expected_revision {
        return Err(stale(&path, expected_revision, actual));
    }
    if matches!(
        &proposal,
        TaskEvent::ReconcileSpec { .. } | TaskEvent::SubtaskReconciled { .. }
    ) {
        let lifecycle = super::LifecycleStore::new(store);
        lifecycle.ensure_task_specification_current(&snapshot.record)?;
    }
    snapshot
        .record
        .apply(expected_revision, proposal.clone())
        .map_err(StoreError::MalformedState)?;
    snapshot.revision = snapshot.record.revision;
    snapshot.updated_at_ms = now_unix_ms();
    let event_path = event_path(event_uid)?;
    let mut event = event_record(
        event_uid,
        &snapshot.record.feature_uid,
        Some(task_uid),
        expected_revision,
        task_actor(&proposal),
        LifecycleEventKind::Task(proposal.clone()),
        super::records::EventProjection::default(),
    )?;
    if let TaskEvent::ReconcileSpec {
        pr_work_item_uid, ..
    } = proposal
    {
        let feature_record_path = feature_path(&snapshot.record.feature_uid);
        let (feature_snapshot, feature_revision) =
            store.read_record::<Snapshot<FeatureGoal>>(&feature_record_path)?;
        if feature_snapshot.record.feature_uid != snapshot.record.feature_uid
            || feature_snapshot.uid != snapshot.record.feature_uid
            || feature_snapshot.revision != feature_revision
            || !feature_snapshot
                .record
                .task_uids
                .contains(&task_uid.to_owned())
        {
            return Err(StoreError::MalformedState(
                "PR preparation requires the current task in its approved feature graph".into(),
            ));
        }
        let mut included_task_uids = vec![task_uid.to_owned()];
        let mut discovered_subtasks = Vec::new();
        for child_uid in &feature_snapshot.record.task_uids {
            if child_uid == task_uid {
                continue;
            }
            let child_path = task_path(child_uid);
            let (child_snapshot, child_revision) =
                store.read_record::<Snapshot<TaskRecord>>(&child_path)?;
            let child = child_snapshot.record;
            if child_snapshot.uid != *child_uid
                || child_revision != child_snapshot.revision
                || child.revision != child_revision
                || child.feature_uid != snapshot.record.feature_uid
            {
                return Err(StoreError::MalformedState(
                    "feature task graph contains an inconsistent subtask record".into(),
                ));
            }
            if child.parent_task_uid.as_deref() == Some(task_uid) {
                if !snapshot.record.subtask_uids.contains(child_uid) {
                    return Err(StoreError::MalformedState(
                        "subtask ownership is missing from its parent record".into(),
                    ));
                }
                if child.phase != TaskPhase::Done || child.pr_work_item_uid.is_some() {
                    return Err(StoreError::MalformedState(
                        "parent task cannot complete before all subtasks are done".into(),
                    ));
                }
                child.validate().map_err(StoreError::MalformedState)?;
                included_task_uids.push(child_uid.clone());
                discovered_subtasks.push(child_uid.clone());
            }
        }
        if discovered_subtasks.len() != snapshot.record.subtask_uids.len()
            || snapshot
                .record
                .subtask_uids
                .iter()
                .any(|uid| !discovered_subtasks.contains(uid))
        {
            return Err(StoreError::MalformedState(
                "parent task graph references missing or misowned subtasks".into(),
            ));
        }
        let now = now_unix_ms();
        let preparation = PullRequestPreparation {
            schema_version: 1,
            uid: pr_work_item_uid.clone(),
            work_item_uid: pr_work_item_uid.clone(),
            feature_uid: snapshot.record.feature_uid.clone(),
            task_uid: task_uid.to_owned(),
            included_task_uids,
            revision: 1,
            created_at_ms: now,
            updated_at_ms: now,
            status: PullRequestPreparationStatus::AwaitingAuthorization,
            publication_authorization: None,
            recovery: None,
            provider_pr_id: None,
            provider_pr_url: None,
            head_sha: None,
            base_sha: None,
            merged_commit_sha: None,
        };
        preparation.validate()?;
        let preparation_snapshot = Snapshot {
            uid: pr_work_item_uid.clone(),
            schema_version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            record: preparation,
        };
        event.pr_preparation_after = Some(preparation_snapshot.record.clone());
        let mut additional = super::spec_sync_queue::complete_for_task(
            store,
            &snapshot.record.feature_uid,
            task_uid,
        )?;
        additional.changes.push((
            pr_work_item_path(&pr_work_item_uid)?,
            encode_record(&preparation_snapshot)?,
        ));
        append_transition_with_additional_checks(
            store,
            &path,
            &snapshot,
            expected_revision,
            event_path,
            event,
            additional,
        )?;
    } else {
        super::records::append_transition(
            store,
            &path,
            &snapshot,
            expected_revision,
            event_path,
            event,
        )?;
    }
    Ok(snapshot.record)
}
