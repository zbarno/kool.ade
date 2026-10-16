use super::{FeatureEvent, LifecycleStore, TaskRecord};
use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{FeaturePhase, TaskPhase},
};

impl LifecycleStore<'_> {
    /// Record a persisted task batch under the exact approved plan. Replays
    /// resume missing task records and accept already advanced matching tasks.
    pub fn reconcile_task_batch(
        &self,
        feature_uid: &str,
        approved_plan_sha256: &str,
        tasks: &[TaskRecord],
    ) -> Result<(), StoreError> {
        if tasks.is_empty() || tasks.len() > 256 {
            return Err(StoreError::MalformedState(
                "approved task batch must contain between 1 and 256 tasks".into(),
            ));
        }
        let feature = self.load_feature(feature_uid)?;
        if feature.approved_plan_sha256.as_deref() != Some(approved_plan_sha256)
            || feature.plan_snapshot_sha256.as_deref() != Some(approved_plan_sha256)
        {
            return Err(StoreError::MalformedState(
                "task batch is not bound to the current approved plan snapshot".into(),
            ));
        }
        let mut task_uids = std::collections::BTreeSet::new();
        let mut planning_uids = std::collections::BTreeSet::new();
        for task in tasks {
            task.validate().map_err(StoreError::MalformedState)?;
            if task.feature_uid != feature_uid
                || task.approved_plan_sha256 != approved_plan_sha256
                || task.phase != TaskPhase::Ready
                || task.revision != 0
                || task.planning_item_uid.is_none()
                || task.task_spec_sha256.is_none()
                || !task_uids.insert(task.task_uid.clone())
                || !planning_uids.insert(task.planning_item_uid.clone().unwrap_or_default())
            {
                return Err(StoreError::MalformedState(
                    "task batch contains missing, duplicate, or differently bound identities"
                        .into(),
                ));
            }
        }
        let mut children = std::collections::BTreeMap::<String, Vec<String>>::new();
        for task in tasks {
            for dependency in &task.dependency_task_uids {
                if !task_uids.contains(dependency) {
                    return Err(StoreError::MalformedState(
                        "task dependency points outside the approved batch".into(),
                    ));
                }
            }
            if let Some(parent_uid) = task.parent_task_uid.as_ref() {
                if !task_uids.contains(parent_uid) || parent_uid == &task.task_uid {
                    return Err(StoreError::MalformedState(
                        "task parent points outside the approved batch".into(),
                    ));
                }
                children
                    .entry(parent_uid.clone())
                    .or_default()
                    .push(task.task_uid.clone());
            }
        }
        for task in tasks {
            let expected_children = children.remove(&task.task_uid).unwrap_or_default();
            if expected_children
                .iter()
                .any(|child| !task.subtask_uids.contains(child))
                || task.subtask_uids.iter().any(|child| {
                    tasks.iter().any(|candidate| {
                        candidate.task_uid == *child
                            && candidate.parent_task_uid.as_deref() != Some(task.task_uid.as_str())
                    })
                })
            {
                return Err(StoreError::MalformedState(
                    "task parent and subtask references do not agree".into(),
                ));
            }
        }
        if feature.task_uids.iter().any(|uid| !task_uids.contains(uid)) {
            return Err(StoreError::MalformedState(
                "lifecycle feature already contains tasks outside this persisted batch".into(),
            ));
        }
        if feature.phase != FeaturePhase::ReadyToDecompose
            && feature.phase != FeaturePhase::Executing
        {
            return Err(StoreError::MalformedState(
                "task batch requires a reviewed plan ready for execution".into(),
            ));
        }
        self.ensure_product_specification_current(feature_uid)?;
        let namespace = uuid::Uuid::parse_str(feature_uid)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        let graph_event_uid = uuid::Uuid::new_v5(
            &namespace,
            format!("{}:task-graph", approved_plan_sha256).as_bytes(),
        )
        .hyphenated()
        .to_string();
        let expected_origins = std::iter::once(graph_event_uid.clone())
            .chain(tasks.iter().map(|task| {
                uuid::Uuid::new_v5(
                    &namespace,
                    format!("{}:task-created:{}", approved_plan_sha256, task.task_uid).as_bytes(),
                )
                .hyphenated()
                .to_string()
            }))
            .collect::<Vec<_>>();
        if self
            .list_spec_sync_queue(feature_uid, false)?
            .iter()
            .any(|entry| {
                entry.trigger != crate::core::lifecycle::SpecSyncTriggerKindV1::TaskGeneration
                    || !expected_origins.contains(&entry.uid)
            })
        {
            return Err(StoreError::MalformedState(
                "task generation has unresolved lifecycle triggers outside this batch".into(),
            ));
        }
        let mut completed = feature
            .task_uids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut order = Vec::with_capacity(tasks.len());
        while order.len() < tasks.len() {
            let next = tasks.iter().enumerate().find(|(index, task)| {
                !order.contains(index)
                    && task
                        .parent_task_uid
                        .as_ref()
                        .is_none_or(|parent| completed.contains(parent))
                    && task
                        .dependency_task_uids
                        .iter()
                        .all(|dependency| completed.contains(dependency))
            });
            let Some((index, task)) = next else {
                return Err(StoreError::MalformedState(
                    "task parent/dependency graph contains a cycle or unresolved predecessor"
                        .into(),
                ));
            };
            order.push(index);
            completed.insert(task.task_uid.clone());
        }
        if feature.phase == FeaturePhase::ReadyToDecompose {
            let namespace = uuid::Uuid::parse_str(feature_uid)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?;
            let event_uid = uuid::Uuid::new_v5(
                &namespace,
                format!("{}:task-graph", approved_plan_sha256).as_bytes(),
            )
            .hyphenated()
            .to_string();
            self.apply_feature_event(
                feature_uid,
                feature.revision,
                &event_uid,
                FeatureEvent::TaskGraphCreated {
                    plan_sha256: approved_plan_sha256.to_owned(),
                },
            )?;
        }
        let namespace = uuid::Uuid::parse_str(feature_uid)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        let graph_event_uid = uuid::Uuid::new_v5(
            &namespace,
            format!("{}:task-graph", approved_plan_sha256).as_bytes(),
        )
        .hyphenated()
        .to_string();
        let mut task_generation_events = vec![graph_event_uid];
        for index in order {
            let task = &tasks[index];
            let event_uid = uuid::Uuid::new_v5(
                &namespace,
                format!("{}:task-created:{}", approved_plan_sha256, task.task_uid).as_bytes(),
            )
            .hyphenated()
            .to_string();
            task_generation_events.push(event_uid.clone());
            self.create_task(task, &event_uid)?;
        }
        let current_feature = self.load_feature(feature_uid)?;
        super::spec_sync::record_approved_plan_revision(
            self,
            &current_feature,
            &task_generation_events,
        )?;
        Ok(())
    }
}
