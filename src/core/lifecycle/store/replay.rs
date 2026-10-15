use super::{FeatureGoal, LifecycleEventKind, LifecycleStore};
use crate::{artifacts::planning_store::StoreError, core::lifecycle::FeaturePhase};
use std::collections::HashMap;

mod event_ownership;
mod pull_request;
mod task;

impl LifecycleStore<'_> {
    /// Rebuilds the feature projection from replayable v3 lifecycle events and
    /// verifies that the result matches the current durable snapshot.
    pub fn replay_feature(&self, feature_uid: &str) -> Result<FeatureGoal, StoreError> {
        let events = self.events_for_feature(feature_uid)?;
        let mut created = events
            .iter()
            .filter(|event| matches!(event.event, LifecycleEventKind::FeatureCreated));
        let creation = created.next().ok_or_else(|| {
            StoreError::MalformedState("feature event history has no creation event".into())
        })?;
        if created.next().is_some()
            || creation.schema_version < 2
            || creation.expected_revision != 0
            || creation.feature_revision_after != Some(1)
            || creation.actor != super::LifecycleActor::Manager
            || creation.initial_task.is_some()
        {
            return Err(StoreError::MalformedState(
                "feature creation event is not replayable".into(),
            ));
        }
        let mut feature = creation.initial_feature.clone().ok_or_else(|| {
            StoreError::MalformedState(
                "feature history predates replayable creation records".into(),
            )
        })?;
        if feature.feature_uid != feature_uid
            || feature.revision != 0
            || feature.phase != FeaturePhase::Exploring
            || !feature.task_uids.is_empty()
        {
            return Err(StoreError::MalformedState(
                "feature creation event contains an invalid initial goal".into(),
            ));
        }
        feature.validate().map_err(StoreError::MalformedState)?;
        feature.revision = 1;
        let mut updated_at_ms = creation.updated_at_ms;
        let mut feature_history = vec![(feature.clone(), updated_at_ms)];
        let mut replayed_task_parents = HashMap::new();

        for event in &events {
            if matches!(
                event.event,
                LifecycleEventKind::FeatureCreated
                    | LifecycleEventKind::Feature(_)
                    | LifecycleEventKind::TaskCreated
                    | LifecycleEventKind::SubtaskCreated { .. }
            ) && event.feature_revision_after.is_none()
            {
                return Err(StoreError::MalformedState(
                    "replayable lifecycle event is missing its feature revision".into(),
                ));
            }
        }

        let mut transitions = events
            .iter()
            .filter(|event| event.uid != creation.uid && event.feature_revision_after.is_some())
            .collect::<Vec<_>>();
        transitions.sort_by_key(|event| event.feature_revision_after);
        for record in transitions {
            updated_at_ms = updated_at_ms.max(record.updated_at_ms);
            let next_revision = feature.revision.checked_add(1).ok_or_else(|| {
                StoreError::MalformedState("feature revision is exhausted".into())
            })?;
            if record.feature_revision_after != Some(next_revision) {
                return Err(StoreError::MalformedState(
                    "feature event history has a missing or duplicated revision".into(),
                ));
            }
            match &record.event {
                LifecycleEventKind::Feature(event) => {
                    if record.expected_revision != feature.revision
                        || record.task_uid.is_some()
                        || record.initial_feature.is_some()
                        || record.initial_task.is_some()
                        || record.actor != super::records::feature_actor(event)
                    {
                        return Err(StoreError::MalformedState(
                            "feature event revision does not match its replay position".into(),
                        ));
                    }
                    feature
                        .apply(feature.revision, event.clone())
                        .map_err(StoreError::MalformedState)?;
                }
                LifecycleEventKind::TaskCreated | LifecycleEventKind::SubtaskCreated { .. } => {
                    replay_task_creation(record, &mut feature, &mut replayed_task_parents)?;
                }
                _ => {
                    return Err(StoreError::MalformedState(
                        "event claims a feature revision without a feature transition".into(),
                    ));
                }
            }
            if feature.revision != next_revision {
                return Err(StoreError::MalformedState(
                    "feature reducer produced an unexpected revision".into(),
                ));
            }
            feature_history.push((feature.clone(), updated_at_ms));
        }

        self.verify_or_restore_feature(
            feature_uid,
            &feature_history,
            creation.created_at_ms,
            false,
        )?;
        let tasks = feature
            .task_uids
            .iter()
            .map(|task_uid| task::replay_task(self, &events, feature_uid, task_uid, false))
            .collect::<Result<Vec<_>, _>>()?;
        event_ownership::validate_events(&events, &tasks)?;
        for task in &tasks {
            pull_request::replay_for_task(self, &events, task, &tasks, false)?;
        }

        self.verify_or_restore_feature(
            feature_uid,
            &feature_history,
            creation.created_at_ms,
            true,
        )?;
        let restored_tasks = feature
            .task_uids
            .iter()
            .map(|task_uid| task::replay_task(self, &events, feature_uid, task_uid, true))
            .collect::<Result<Vec<_>, _>>()?;
        for task in &restored_tasks {
            pull_request::replay_for_task(self, &events, task, &restored_tasks, true)?;
        }
        Ok(feature)
    }

    fn verify_or_restore_feature(
        &self,
        feature_uid: &str,
        history: &[(FeatureGoal, u64)],
        created_at_ms: u64,
        restore_snapshot: bool,
    ) -> Result<(), StoreError> {
        let current = match self.load_feature(feature_uid) {
            Ok(snapshot) => snapshot,
            Err(StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound && !restore_snapshot =>
            {
                return Ok(());
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                let (initial, updated_at_ms) = history
                    .first()
                    .ok_or_else(|| StoreError::MalformedState("feature history is empty".into()))?;
                let snapshot = super::Snapshot {
                    uid: feature_uid.to_owned(),
                    schema_version: 1,
                    created_at_ms,
                    updated_at_ms: *updated_at_ms,
                    revision: initial.revision,
                    record: initial.clone(),
                };
                let path = super::records::feature_path(feature_uid);
                match self.store.save_record(&path, &snapshot, 0) {
                    Ok(_) => initial.clone(),
                    Err(StoreError::StaleRecordRevision { .. }) => {
                        self.load_feature(feature_uid).map_err(|_| {
                            StoreError::MalformedState(
                                "concurrent feature recovery produced no readable snapshot".into(),
                            )
                        })?
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        let mut index = history
            .iter()
            .position(|(feature, _)| *feature == current)
            .ok_or_else(|| {
                StoreError::MalformedState(
                    "replayed feature does not match its durable snapshot".into(),
                )
            })?;
        if !restore_snapshot {
            return Ok(());
        }
        while index + 1 < history.len() {
            let (feature, updated_at_ms) = &history[index + 1];
            let snapshot = super::Snapshot {
                uid: feature_uid.to_owned(),
                schema_version: 1,
                created_at_ms,
                updated_at_ms: *updated_at_ms,
                revision: feature.revision,
                record: feature.clone(),
            };
            let path = super::records::feature_path(feature_uid);
            match self
                .store
                .save_record(&path, &snapshot, history[index].0.revision)
            {
                Ok(_) => index += 1,
                Err(StoreError::StaleRecordRevision { .. }) => {
                    let concurrent = self.load_feature(feature_uid)?;
                    let Some(concurrent_index) =
                        history.iter().position(|(state, _)| *state == concurrent)
                    else {
                        return Err(StoreError::MalformedState(
                            "concurrent feature recovery produced a conflicting snapshot".into(),
                        ));
                    };
                    if concurrent_index <= index {
                        return Err(StoreError::MalformedState(
                            "concurrent feature recovery did not advance the snapshot".into(),
                        ));
                    }
                    index = concurrent_index;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn replay_task_creation(
    event: &super::LifecycleEventRecord,
    feature: &mut FeatureGoal,
    task_parents: &mut HashMap<String, Option<String>>,
) -> Result<(), StoreError> {
    if feature.phase != FeaturePhase::Executing
        || event.expected_revision != 0
        || event.actor != super::LifecycleActor::ApplicationPolicy
        || event.initial_feature.is_some()
    {
        return Err(StoreError::MalformedState(
            "task creation occurred outside an executing feature".into(),
        ));
    }
    let task_uid = event.task_uid.as_deref().ok_or_else(|| {
        StoreError::MalformedState("task creation event has no task identity".into())
    })?;
    let task = event.initial_task.as_ref().ok_or_else(|| {
        StoreError::MalformedState("task creation event has no initial task record".into())
    })?;
    task.validate().map_err(StoreError::MalformedState)?;
    if task.task_uid != task_uid
        || task.feature_uid != feature.feature_uid
        || task.revision != 0
        || task.approved_plan_sha256 != feature.approved_plan_sha256.as_deref().unwrap_or_default()
        || feature.task_uids.iter().any(|uid| uid == task_uid)
        || task.phase != crate::core::lifecycle::TaskPhase::Ready
    {
        return Err(StoreError::MalformedState(
            "task creation event does not match the approved feature graph".into(),
        ));
    }
    match &event.event {
        LifecycleEventKind::TaskCreated if task.parent_task_uid.is_none() => {}
        LifecycleEventKind::SubtaskCreated { parent_task_uid }
            if task.parent_task_uid.as_deref() == Some(parent_task_uid)
                && task_parents
                    .get(parent_task_uid)
                    .is_some_and(Option::is_none) => {}
        _ => {
            return Err(StoreError::MalformedState(
                "task creation event does not match its parent relationship".into(),
            ));
        }
    }
    feature.task_uids.push(task_uid.to_owned());
    task_parents.insert(task_uid.to_owned(), task.parent_task_uid.clone());
    feature.revision += 1;
    feature.validate().map_err(StoreError::MalformedState)
}
