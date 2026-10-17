use super::*;
use std::sync::{Arc, Barrier};

#[test]
// Invariant: concurrent writes at one sequence yield one commit and one CAS rejection.
fn concurrent_distinct_revisions_cannot_overwrite_each_other() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let left = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "concurrent-left")
        .unwrap();
    let right = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "concurrent-right")
        .unwrap();
    let left_draft = Fixture::draft(&store, 1, left.uid);
    let right_draft = Fixture::draft(&store, 1, right.uid);
    let barrier = Arc::new(Barrier::new(2));
    let spawn = |store: PlanningStore,
                 feature_uid: String,
                 draft: SpecRevisionDraftV1,
                 barrier: Arc<Barrier>| {
        std::thread::spawn(move || {
            barrier.wait();
            LifecycleStore::new(&store).append_spec_revision(&feature_uid, 0, draft)
        })
    };
    let left_thread = spawn(
        store.clone(),
        feature_uid.clone(),
        left_draft,
        Arc::clone(&barrier),
    );
    let right_thread = spawn(
        store.clone(),
        feature_uid.clone(),
        right_draft,
        Arc::clone(&barrier),
    );
    let outcomes = [left_thread.join().unwrap(), right_thread.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(StoreError::StaleRecordRevision { .. })))
            .count(),
        1
    );
    assert_eq!(
        LifecycleStore::new(&store)
            .spec_revision_history(&feature_uid)
            .unwrap()
            .len(),
        1
    );
    let lifecycle = LifecycleStore::new(&store);
    let dispositions = lifecycle
        .specification_origin_dispositions(&feature_uid, 1)
        .unwrap()
        .unwrap();
    assert_eq!(dispositions.len(), 1);
    let winning_origin = &dispositions[0].origin_event_uid;
    let queue = lifecycle.list_spec_sync_queue(&feature_uid, true).unwrap();
    assert_eq!(
        queue
            .iter()
            .filter(|entry| entry.status == SpecSyncQueueStatusV1::Completed)
            .count(),
        1
    );
    assert!(
        queue.iter().any(|entry| &entry.uid == winning_origin
            && entry.status == SpecSyncQueueStatusV1::Completed)
    );
}
