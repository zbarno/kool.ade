use serde::Serialize;

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::super::types::is_uuid;
use super::{
    FeatureEvent, LifecycleActor, LifecycleEventKind, LifecycleEventRecord, Snapshot, TaskEvent,
};

pub(super) struct AdditionalTransition {
    pub changes: Vec<(String, Vec<u8>)>,
    pub checks: Vec<RecordRevisionCheck>,
}

pub(super) fn create_pair<T: Serialize>(
    store: &PlanningStore,
    snapshot_path: String,
    snapshot: &Snapshot<T>,
    event_path: String,
    event: LifecycleEventRecord,
) -> Result<(), StoreError> {
    create_pair_with_checks(store, snapshot_path, snapshot, event_path, event, &[])
}

pub(super) fn create_pair_with_checks<T: Serialize>(
    store: &PlanningStore,
    snapshot_path: String,
    snapshot: &Snapshot<T>,
    event_path: String,
    event: LifecycleEventRecord,
    extra_checks: &[RecordRevisionCheck],
) -> Result<(), StoreError> {
    create_pair_with_extra(
        store,
        snapshot_path,
        snapshot,
        event_path,
        event,
        Vec::new(),
        extra_checks,
    )
}

pub(super) fn create_pair_with_extra<T: Serialize>(
    store: &PlanningStore,
    snapshot_path: String,
    snapshot: &Snapshot<T>,
    event_path: String,
    event: LifecycleEventRecord,
    extra_changes: Vec<(String, Vec<u8>)>,
    extra_checks: &[RecordRevisionCheck],
) -> Result<(), StoreError> {
    let mut changes = vec![
        (snapshot_path.clone(), encode(snapshot)?),
        (event_path.clone(), encode(&event)?),
    ];
    changes.extend(extra_changes);
    let mut checks = vec![
        RecordRevisionCheck {
            path: snapshot_path,
            expected_revision: 0,
        },
        RecordRevisionCheck {
            path: event_path,
            expected_revision: 0,
        },
    ];
    checks.extend_from_slice(extra_checks);
    store
        .transaction_with_record_revisions(&changes, &checks)
        .map(|_| ())
}

pub(super) fn append_transition<T: Serialize>(
    store: &PlanningStore,
    path: &str,
    snapshot: &Snapshot<T>,
    expected_revision: u64,
    event_path: String,
    event: LifecycleEventRecord,
) -> Result<(), StoreError> {
    let changes = vec![
        (path.to_owned(), encode(snapshot)?),
        (event_path.clone(), encode(&event)?),
    ];
    let checks = [
        RecordRevisionCheck {
            path: path.to_owned(),
            expected_revision,
        },
        RecordRevisionCheck {
            path: event_path,
            expected_revision: 0,
        },
    ];
    store
        .transaction_with_record_revisions(&changes, &checks)
        .map(|_| ())
}

pub(super) fn append_transition_with_additional_checks<T: Serialize>(
    store: &PlanningStore,
    path: &str,
    snapshot: &Snapshot<T>,
    expected_revision: u64,
    event_path: String,
    event: LifecycleEventRecord,
    additional: AdditionalTransition,
) -> Result<(), StoreError> {
    let mut changes = vec![
        (path.to_owned(), encode(snapshot)?),
        (event_path.clone(), encode(&event)?),
    ];
    let mut checks = vec![
        RecordRevisionCheck {
            path: path.to_owned(),
            expected_revision,
        },
        RecordRevisionCheck {
            path: event_path,
            expected_revision: 0,
        },
    ];
    for (path, contents) in additional.changes {
        checks.push(RecordRevisionCheck {
            path: path.clone(),
            expected_revision: 0,
        });
        changes.push((path, contents));
    }
    checks.extend(additional.checks);
    store
        .transaction_with_record_revisions(&changes, &checks)
        .map(|_| ())
}

pub(super) fn encode_record<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    encode(value)
}

pub(super) fn event_record(
    uid: &str,
    feature_uid: &str,
    task_uid: Option<&str>,
    expected_revision: u64,
    actor: LifecycleActor,
    event: LifecycleEventKind,
) -> Result<LifecycleEventRecord, StoreError> {
    if !is_uuid(uid) || !is_uuid(feature_uid) || task_uid.is_some_and(|id| !is_uuid(id)) {
        return Err(StoreError::InvalidPath(uid.to_owned()));
    }
    let now = now_unix_ms();
    Ok(LifecycleEventRecord {
        uid: uid.to_owned(),
        schema_version: 1,
        created_at_ms: now,
        updated_at_ms: now,
        revision: 1,
        feature_uid: feature_uid.to_owned(),
        task_uid: task_uid.map(str::to_owned),
        expected_revision,
        actor,
        event,
    })
}

pub(super) fn feature_actor(event: &FeatureEvent) -> LifecycleActor {
    match event {
        FeatureEvent::AnswersResolved
        | FeatureEvent::UserPlanDecision(_)
        | FeatureEvent::Cancel => LifecycleActor::User,
        FeatureEvent::PlanReview(_) => LifecycleActor::IndependentReviewer,
        FeatureEvent::TaskGraphCreated { .. } | FeatureEvent::TasksCompleted => {
            LifecycleActor::ApplicationPolicy
        }
        FeatureEvent::QuestionsRaised { .. } | FeatureEvent::PlanReady { .. } => {
            LifecycleActor::Manager
        }
        FeatureEvent::DraftUpdated { .. } | FeatureEvent::ReadinessRecorded { .. } => {
            LifecycleActor::Manager
        }
    }
}

pub(super) fn task_actor(event: &TaskEvent) -> LifecycleActor {
    match event {
        TaskEvent::CodeReview { .. } => LifecycleActor::IndependentReviewer,
        TaskEvent::RuntimeQaRequested(_)
        | TaskEvent::RuntimeQaBlocked { .. }
        | TaskEvent::RuntimeQaUnavailable { .. }
        | TaskEvent::SpecReconciliationBlocked { .. } => LifecycleActor::ApplicationPolicy,
        TaskEvent::RuntimeQaCompleted { .. } => LifecycleActor::IndependentReviewer,
        TaskEvent::ReconcileSpec { .. } | TaskEvent::SubtaskReconciled { .. } => {
            LifecycleActor::ApplicationPolicy
        }
        TaskEvent::SpecReconciliationRetry { .. } => LifecycleActor::ApplicationPolicy,
        TaskEvent::Start | TaskEvent::ImplementationFinished { .. } | TaskEvent::BeginRepair => {
            LifecycleActor::Worker
        }
        TaskEvent::Cancel => LifecycleActor::User,
    }
}

pub(super) fn feature_path(uid: &str) -> String {
    format!("state/lifecycle_features/{uid}.json")
}

pub(super) fn task_path(uid: &str) -> String {
    format!("state/lifecycle_tasks/{uid}.json")
}

pub(super) fn pr_work_item_path(uid: &str) -> Result<String, StoreError> {
    if !is_uuid(uid) {
        return Err(StoreError::InvalidPath(uid.to_owned()));
    }
    Ok(format!("state/lifecycle_pr_work_items/{uid}.json"))
}

pub(super) fn event_path(uid: &str) -> Result<String, StoreError> {
    if !is_uuid(uid) {
        return Err(StoreError::InvalidPath(uid.to_owned()));
    }
    Ok(format!("state/lifecycle_events/{uid}.json"))
}

pub(super) fn stale(path: &str, expected: u64, actual: u64) -> StoreError {
    StoreError::StaleRecordRevision {
        path: path.into(),
        expected,
        actual,
    }
}

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec_pretty(value).map_err(|error| {
        StoreError::MalformedState(format!("lifecycle record cannot be encoded: {error}"))
    })
}
