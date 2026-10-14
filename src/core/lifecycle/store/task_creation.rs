use super::records::{
    create_pair_with_extra, event_path, event_record, feature_path, now_unix_ms, stale, task_path,
};
use super::{
    FeatureGoal, LifecycleActor, LifecycleEventKind, LifecycleStore, Snapshot, TaskRecord,
};
use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{FeaturePhase, TaskPhase},
};

impl LifecycleStore<'_> {
    pub fn create_task(&self, task: &TaskRecord, event_uid: &str) -> Result<(), StoreError> {
        task.validate().map_err(StoreError::MalformedState)?;
        let feature = self.load_feature(&task.feature_uid)?;
        if feature.phase != FeaturePhase::Executing
            || feature.approved_plan_sha256.as_deref() != Some(task.approved_plan_sha256.as_str())
        {
            return Err(StoreError::MalformedState(
                "task creation requires the feature's reviewed approved plan".into(),
            ));
        }
        if feature.task_uids.contains(&task.task_uid) {
            let existing = self.load_task(&task.task_uid)?;
            let event_exists = self
                .events_for_feature(&task.feature_uid)?
                .iter()
                .any(|event| {
                    event.uid == event_uid
                        && event.task_uid.as_deref() == Some(task.task_uid.as_str())
                });
            if existing.feature_uid == task.feature_uid
                && existing.approved_plan_sha256 == task.approved_plan_sha256
                && existing.planning_item_uid == task.planning_item_uid
                && existing.task_spec_sha256 == task.task_spec_sha256
                && existing.parent_task_uid == task.parent_task_uid
                && existing.subtask_uids == task.subtask_uids
                && existing.dependency_task_uids == task.dependency_task_uids
                && event_exists
            {
                return Ok(());
            }
            return Err(StoreError::MalformedState(
                "existing lifecycle task does not match the retried planning item".into(),
            ));
        }
        if task.revision != 0 || task.phase != TaskPhase::Ready {
            return Err(StoreError::MalformedState(
                "new task must start at revision zero in ready phase".into(),
            ));
        }
        if feature.task_uids.len() >= 256 {
            return Err(StoreError::MalformedState(
                "task graph is full or task identity already exists".into(),
            ));
        }
        let feature_record_path = feature_path(&task.feature_uid);
        let (mut feature_snapshot, actual_feature_revision) = self
            .store
            .read_record::<Snapshot<FeatureGoal>>(&feature_record_path)?;
        if actual_feature_revision != feature.revision
            || feature_snapshot.revision != feature.revision
            || feature_snapshot.record.feature_uid != task.feature_uid
            || task.parent_task_uid.as_deref().is_some_and(|parent_uid| {
                !feature_snapshot
                    .record
                    .task_uids
                    .iter()
                    .any(|uid| uid == parent_uid)
            })
        {
            return Err(stale(
                &feature_record_path,
                feature.revision,
                actual_feature_revision,
            ));
        }
        if feature_snapshot.revision == u64::MAX {
            return Err(StoreError::MalformedState(
                "feature revision is exhausted".into(),
            ));
        }
        let mut parent_update = None;
        if let Some(parent_uid) = task.parent_task_uid.as_deref() {
            let parent_record_path = task_path(parent_uid);
            let (mut parent_snapshot, parent_revision) = self
                .store
                .read_record::<Snapshot<TaskRecord>>(&parent_record_path)?;
            if parent_snapshot.uid != parent_uid
                || parent_snapshot.record.task_uid != parent_uid
                || parent_snapshot.record.feature_uid != task.feature_uid
                || parent_snapshot.record.approved_plan_sha256 != task.approved_plan_sha256
                || parent_snapshot.record.parent_task_uid.is_some()
                || parent_snapshot.record.phase != TaskPhase::Ready
                || parent_snapshot.record.revision != parent_revision
                || parent_snapshot.revision != parent_revision
                || parent_revision == u64::MAX
            {
                return Err(StoreError::MalformedState(
                    "subtask parent must be a ready standalone task in the same approved plan"
                        .into(),
                ));
            }
            if !parent_snapshot.record.subtask_uids.contains(&task.task_uid) {
                if parent_snapshot.record.subtask_uids.len() >= 256 {
                    return Err(StoreError::MalformedState(
                        "subtask parent already owns the maximum number of subtasks".into(),
                    ));
                }
                parent_snapshot
                    .record
                    .subtask_uids
                    .push(task.task_uid.clone());
                parent_snapshot.record.revision += 1;
                parent_snapshot.revision = parent_snapshot.record.revision;
                parent_snapshot.updated_at_ms = now_unix_ms();
                parent_snapshot
                    .record
                    .validate()
                    .map_err(StoreError::MalformedState)?;
                parent_update = Some((parent_record_path, parent_snapshot, parent_revision));
            }
        }
        feature_snapshot
            .record
            .task_uids
            .push(task.task_uid.clone());
        feature_snapshot.record.revision += 1;
        feature_snapshot.revision = feature_snapshot.record.revision;
        feature_snapshot.updated_at_ms = now_unix_ms();
        feature_snapshot
            .record
            .validate()
            .map_err(StoreError::MalformedState)?;
        let now = now_unix_ms();
        let snapshot_path = task_path(&task.task_uid);
        let event_path = event_path(event_uid)?;
        let mut initial = task.clone();
        initial.revision = 1;
        let snapshot = Snapshot {
            uid: task.task_uid.clone(),
            schema_version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            revision: initial.revision,
            record: initial,
        };
        let event = event_record(
            event_uid,
            &task.feature_uid,
            Some(&task.task_uid),
            0,
            LifecycleActor::ApplicationPolicy,
            task.parent_task_uid.as_ref().map_or(
                LifecycleEventKind::TaskCreated,
                |parent_task_uid| LifecycleEventKind::SubtaskCreated {
                    parent_task_uid: parent_task_uid.clone(),
                },
            ),
        )?;
        let feature_check = crate::artifacts::planning_store::RecordRevisionCheck {
            path: feature_record_path.clone(),
            expected_revision: feature.revision,
        };
        let mut extra_writes = vec![(
            feature_record_path.clone(),
            serde_json::to_vec_pretty(&feature_snapshot).map_err(|error| {
                StoreError::MalformedState(format!("feature snapshot cannot be encoded: {error}"))
            })?,
        )];
        let mut checks = vec![feature_check];
        if let Some((parent_path, parent_snapshot, parent_revision)) = parent_update {
            let parent_bytes = serde_json::to_vec_pretty(&parent_snapshot).map_err(|error| {
                StoreError::MalformedState(format!(
                    "parent task snapshot cannot be encoded: {error}"
                ))
            })?;
            extra_writes.push((parent_path.clone(), parent_bytes));
            checks.push(crate::artifacts::planning_store::RecordRevisionCheck {
                path: parent_path,
                expected_revision: parent_revision,
            });
        }
        create_pair_with_extra(
            self.store,
            snapshot_path,
            &snapshot,
            event_path,
            event,
            extra_writes,
            &checks,
        )
    }
}
