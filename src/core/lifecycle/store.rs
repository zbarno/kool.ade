use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::{FeatureEvent, FeatureGoal, TaskEvent, TaskRecord};
mod feature_event;
mod pr_state;
mod reconcile_plan;
mod records;
mod replay;
mod runtime_qa;
mod spec_reconciliation;
mod task_batch;
mod task_creation;
mod types;
use types::Snapshot;
pub use types::{LifecycleActor, LifecycleEventKind, LifecycleEventRecord};
pub(super) mod task_transition;
use records::{
    EventProjection, create_pair, event_path, event_record, feature_path, now_unix_ms, task_path,
};
pub use task_transition::PullRequestPreparation;
pub use task_transition::PullRequestPublicationState;

/// Stores mutable lifecycle snapshots and append-only transition records in the planning repo.
pub struct LifecycleStore<'a> {
    pub(super) store: &'a PlanningStore,
}

impl<'a> LifecycleStore<'a> {
    pub fn new(store: &'a PlanningStore) -> Self {
        Self { store }
    }

    pub fn create_feature(&self, goal: &FeatureGoal, event_uid: &str) -> Result<(), StoreError> {
        goal.validate().map_err(StoreError::MalformedState)?;
        if goal.revision != 0
            || goal.phase != super::FeaturePhase::Exploring
            || !goal.task_uids.is_empty()
        {
            return Err(StoreError::MalformedState(
                "new feature must start at revision zero in exploring phase".into(),
            ));
        }
        let now = now_unix_ms();
        let snapshot_path = feature_path(&goal.feature_uid);
        let event_path = event_path(event_uid)?;
        let mut initial = goal.clone();
        initial.revision = 1;
        let snapshot = Snapshot {
            uid: goal.feature_uid.clone(),
            schema_version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            revision: initial.revision,
            record: initial,
        };
        let event = event_record(
            event_uid,
            &goal.feature_uid,
            None,
            0,
            LifecycleActor::Manager,
            LifecycleEventKind::FeatureCreated,
            EventProjection {
                feature_revision_after: Some(1),
                initial_feature: Some(goal.clone()),
                ..EventProjection::default()
            },
        )?;
        create_pair(self.store, snapshot_path, &snapshot, event_path, event)
    }

    pub fn load_feature(&self, feature_uid: &str) -> Result<FeatureGoal, StoreError> {
        let path = feature_path(feature_uid);
        let (snapshot, _) = self.store.read_record::<Snapshot<FeatureGoal>>(&path)?;
        if snapshot.uid != feature_uid || snapshot.uid != snapshot.record.feature_uid {
            return Err(StoreError::MalformedState(
                "feature identity does not match its planning record".into(),
            ));
        }
        if snapshot.revision != snapshot.record.revision {
            return Err(StoreError::MalformedState(
                "feature record revision does not match its snapshot".into(),
            ));
        }
        snapshot
            .record
            .validate()
            .map_err(StoreError::MalformedState)?;
        Ok(snapshot.record)
    }

    pub fn load_task(&self, task_uid: &str) -> Result<TaskRecord, StoreError> {
        let path = task_path(task_uid);
        let (snapshot, _) = self.store.read_record::<Snapshot<TaskRecord>>(&path)?;
        if snapshot.uid != task_uid || snapshot.uid != snapshot.record.task_uid {
            return Err(StoreError::MalformedState(
                "task identity does not match its planning record".into(),
            ));
        }
        if snapshot.revision != snapshot.record.revision {
            return Err(StoreError::MalformedState(
                "task record revision does not match its snapshot".into(),
            ));
        }
        snapshot
            .record
            .validate()
            .map_err(StoreError::MalformedState)?;
        Ok(snapshot.record)
    }

    /// Loads the validated lifecycle task snapshots in one planning-store pass.
    pub fn list_tasks(&self) -> Result<Vec<TaskRecord>, StoreError> {
        records::load_tasks(self.store)
    }

    /// Resolve the lifecycle identity for a persisted planning task without
    /// assuming that its path or display key equals the lifecycle UUID.
    pub fn task_for_planning_item(
        &self,
        planning_item_uid: &str,
    ) -> Result<Option<TaskRecord>, StoreError> {
        let mut matches = self
            .list_tasks()?
            .into_iter()
            .filter(|task| task.planning_item_uid.as_deref() == Some(planning_item_uid))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(StoreError::MalformedState(
                "planning item maps to multiple lifecycle tasks".into(),
            ));
        }
        Ok(matches.pop())
    }

    pub fn load_pr_preparation(
        &self,
        work_item_uid: &str,
    ) -> Result<PullRequestPreparation, StoreError> {
        task_transition::load(self.store, work_item_uid)
    }

    pub fn authorize_pr_publication(
        &self,
        work_item_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        receipt: &crate::core::lifecycle::GateReceipt,
    ) -> Result<PullRequestPreparation, StoreError> {
        pr_state::authorize(
            self.store,
            work_item_uid,
            expected_revision,
            event_uid,
            receipt,
        )
    }

    pub fn record_pr_publication_state(
        &self,
        work_item_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        update: PullRequestPublicationState,
    ) -> Result<PullRequestPreparation, StoreError> {
        pr_state::record_state(
            self.store,
            work_item_uid,
            expected_revision,
            event_uid,
            update,
        )
    }

    pub fn events_for_feature(
        &self,
        feature_uid: &str,
    ) -> Result<Vec<LifecycleEventRecord>, StoreError> {
        let mut events = self
            .store
            .list_record_paths("state/lifecycle_events")?
            .into_iter()
            .map(|path| {
                self.store
                    .read_record::<LifecycleEventRecord>(&path)
                    .map(|(event, _)| event)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if events.iter().any(|event| {
            !matches!(event.schema_version, 1..=3)
                || event.revision != 1
                || !super::types::is_uuid(&event.feature_uid)
                || event
                    .task_uid
                    .as_deref()
                    .is_some_and(|uid| !super::types::is_uuid(uid))
                || matches!(
                    &event.event,
                    LifecycleEventKind::SubtaskCreated { parent_task_uid }
                        if !super::types::is_uuid(parent_task_uid)
                )
                || matches!(
                    &event.event,
                    LifecycleEventKind::PullRequestState { work_item_uid, .. }
                        if !super::types::is_uuid(work_item_uid)
                )
                || event.created_at_ms == 0
                || event.updated_at_ms < event.created_at_ms
                || event.feature_revision_after == Some(0)
        }) {
            return Err(StoreError::MalformedState(
                "lifecycle event record is malformed".into(),
            ));
        }
        events.retain(|event| event.feature_uid == feature_uid);
        events.sort_by(|left, right| {
            (left.created_at_ms, &left.uid).cmp(&(right.created_at_ms, &right.uid))
        });
        Ok(events)
    }

    pub fn apply_feature_event(
        &self,
        feature_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        proposal: FeatureEvent,
    ) -> Result<FeatureGoal, StoreError> {
        feature_event::apply(
            self.store,
            feature_uid,
            expected_revision,
            event_uid,
            proposal,
        )
    }

    pub fn apply_task_event(
        &self,
        task_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        proposal: TaskEvent,
    ) -> Result<TaskRecord, StoreError> {
        task_transition::apply_task_event(
            self.store,
            task_uid,
            expected_revision,
            event_uid,
            proposal,
        )
    }
}
