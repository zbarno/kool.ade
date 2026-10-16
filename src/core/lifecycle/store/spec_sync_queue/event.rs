use super::require_uuid;
use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        FeatureEvent, LifecycleEventKind, LifecycleEventRecord, SpecSyncQueueEntryV1,
        SpecSyncQueueStatusV1, SpecSyncTriggerKindV1, TaskEvent,
    },
};

pub(in crate::core::lifecycle::store) fn queued_for_event(
    event: &LifecycleEventRecord,
) -> Result<SpecSyncQueueEntryV1, StoreError> {
    let (trigger, declined) = trigger_for_event(event);
    let completed = matches!(
        &event.event,
        LifecycleEventKind::Task(
            TaskEvent::ReconcileSpec { .. } | TaskEvent::SubtaskReconciled { .. }
        )
    );
    let entry = SpecSyncQueueEntryV1 {
        schema_version: 1,
        uid: event.uid.clone(),
        feature_uid: event.feature_uid.clone(),
        source_event_uid: event.uid.clone(),
        source_task_uid: event.task_uid.clone(),
        request_message_id: None,
        source_revision: event
            .feature_revision_after
            .or(event.task_revision_after)
            .unwrap_or(event.expected_revision.saturating_add(1)),
        trigger,
        status: if declined.is_some() {
            SpecSyncQueueStatusV1::Declined
        } else if completed {
            SpecSyncQueueStatusV1::Completed
        } else {
            SpecSyncQueueStatusV1::Pending
        },
        decline_reason: declined,
        attempts: if completed { 1 } else { 0 },
        revision: 1,
        created_at_ms: event.created_at_ms,
        updated_at_ms: event.updated_at_ms,
    };
    entry.validate().map_err(StoreError::MalformedState)?;
    Ok(entry)
}

pub(in crate::core::lifecycle::store) fn queue_path(uid: &str) -> Result<String, StoreError> {
    require_uuid(uid, "event")?;
    Ok(format!("state/spec_sync_queue/{uid}.json"))
}

fn trigger_for_event(event: &LifecycleEventRecord) -> (SpecSyncTriggerKindV1, Option<String>) {
    match &event.event {
        LifecycleEventKind::FeatureCreated => (SpecSyncTriggerKindV1::FeatureClarification, None),
        LifecycleEventKind::Feature(event) => match event {
            FeatureEvent::QuestionsRaised { .. } | FeatureEvent::AnswersResolved => {
                (SpecSyncTriggerKindV1::FeatureClarification, None)
            }
            FeatureEvent::UserPlanDecision(_) => (SpecSyncTriggerKindV1::UserDecision, None),
            FeatureEvent::PlanReview(_) => (SpecSyncTriggerKindV1::PlanReview, None),
            FeatureEvent::TaskGraphCreated { .. } => (SpecSyncTriggerKindV1::TaskGeneration, None),
            FeatureEvent::DraftUpdated { .. }
            | FeatureEvent::ReadinessRecorded { .. }
            | FeatureEvent::PlanReady { .. }
            | FeatureEvent::TasksCompleted
            | FeatureEvent::Cancel => (SpecSyncTriggerKindV1::FeatureClarification, None),
        },
        LifecycleEventKind::TaskCreated | LifecycleEventKind::SubtaskCreated { .. } => {
            (SpecSyncTriggerKindV1::TaskGeneration, None)
        }
        LifecycleEventKind::Task(event) => match event {
            TaskEvent::ImplementationFinished { .. } => {
                (SpecSyncTriggerKindV1::Implementation, None)
            }
            TaskEvent::CodeReview { .. } => (SpecSyncTriggerKindV1::CodeReview, None),
            TaskEvent::BeginRepair => (SpecSyncTriggerKindV1::Repair, None),
            TaskEvent::RuntimeQaRequested(_)
            | TaskEvent::RuntimeQaBlocked { .. }
            | TaskEvent::RuntimeQaUnavailable { .. }
            | TaskEvent::RuntimeQaCompleted { .. } => (SpecSyncTriggerKindV1::RuntimeQa, None),
            TaskEvent::ReconcileSpec { .. }
            | TaskEvent::SpecReconciliationBlocked { .. }
            | TaskEvent::SpecReconciliationRetry { .. }
            | TaskEvent::SubtaskReconciled { .. } => {
                (SpecSyncTriggerKindV1::SpecificationReconciliation, None)
            }
            TaskEvent::Start | TaskEvent::Cancel => (
                SpecSyncTriggerKindV1::LifecycleOther,
                Some("task start/cancel does not change reviewed specification evidence".into()),
            ),
        },
        LifecycleEventKind::PullRequestState { .. } => (SpecSyncTriggerKindV1::PullRequest, None),
    }
}
