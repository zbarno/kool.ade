use super::Fixture;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{LifecycleStore, SpecSyncQueueStatusV1, SpecSyncTriggerKindV1};

#[test]
// Invariant: lifecycle events and queue rows commit together; manual refresh
// retries preserve stable identities across an application restart.
fn lifecycle_event_and_manual_refresh_queue_are_durable_and_idempotent() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, event_uid) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let entries = lifecycle.list_spec_sync_queue(&feature_uid, true).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_event_uid, event_uid);
    assert_eq!(
        entries[0].trigger,
        SpecSyncTriggerKindV1::FeatureClarification
    );
    assert_eq!(entries[0].status, SpecSyncQueueStatusV1::Pending);

    let request_message_id = "17aaf010-1-42".to_owned();
    let first = lifecycle
        .enqueue_manual_spec_refresh_correlated(
            &feature_uid,
            "refresh-session-01",
            Some(&request_message_id),
        )
        .unwrap();
    assert_eq!(
        lifecycle
            .enqueue_manual_spec_refresh_correlated(
                &feature_uid,
                "refresh-session-01",
                Some(&request_message_id),
            )
            .unwrap(),
        first
    );
    assert_eq!(
        first.request_message_id.as_deref(),
        Some(request_message_id.as_str())
    );
    let conflicting_message = "17aaf010-2-42";
    assert!(
        lifecycle
            .enqueue_manual_spec_refresh_correlated(
                &feature_uid,
                "refresh-session-01",
                Some(conflicting_message),
            )
            .is_err()
    );

    let reopened = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened_lifecycle = LifecycleStore::new(&reopened);
    assert_eq!(
        reopened_lifecycle
            .enqueue_manual_spec_refresh_correlated(
                &feature_uid,
                "refresh-session-01",
                Some(&request_message_id),
            )
            .unwrap(),
        first
    );
    let restored = reopened_lifecycle
        .list_spec_sync_queue(&feature_uid, true)
        .unwrap();
    assert!(restored.iter().any(|entry| entry.uid == first.uid));
}
