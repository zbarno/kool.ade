use super::*;
use crate::core::lifecycle::FeatureEvent;

#[test]
// Production boundary: task graph/task writes and recovery through the same
// file-backed batch API. Reducer tests cannot establish interrupted-prefix
// recovery. Local PlanningStore and bootstrap only; low setup, no services.
fn interrupted_generation_resumes_its_own_events_but_rejects_unrelated_work() {
    for written_tasks in [0, 1] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let lifecycle = LifecycleStore::new(&store);
        let (feature_uid, hash) = approved_feature(&lifecycle);
        crate::artifacts::migration::bootstrap_product(&fixture.0, "Batch recovery").unwrap();
        record_approved_spec_baseline(&lifecycle, &feature_uid);
        let tasks = [
            task(&feature_uid, &hash, &uuid::Uuid::new_v4().to_string()),
            task(&feature_uid, &hash, &uuid::Uuid::new_v4().to_string()),
        ];
        let namespace = uuid::Uuid::parse_str(&feature_uid).unwrap();
        let event =
            uuid::Uuid::new_v5(&namespace, format!("{hash}:task-graph").as_bytes()).to_string();
        let feature = lifecycle.load_feature(&feature_uid).unwrap();
        lifecycle
            .apply_feature_event(
                &feature_uid,
                feature.revision,
                &event,
                FeatureEvent::TaskGraphCreated {
                    plan_sha256: hash.clone(),
                },
            )
            .unwrap();
        for task in tasks.iter().take(written_tasks) {
            let event = uuid::Uuid::new_v5(
                &namespace,
                format!("{hash}:task-created:{}", task.task_uid).as_bytes(),
            )
            .to_string();
            lifecycle.create_task(task, &event).unwrap();
        }
        let reopened_store = fixture.store();
        let reopened = LifecycleStore::new(&reopened_store);
        let manual = reopened
            .enqueue_manual_spec_refresh(&feature_uid, "unrelated")
            .unwrap();
        assert!(
            reopened
                .reconcile_task_batch(&feature_uid, &hash, &tasks)
                .is_err()
        );
        reopened
            .decline_spec_sync_queue_entry(
                &manual.uid,
                manual.revision,
                "Operator cancelled this unrelated request",
            )
            .unwrap();
        reopened
            .reconcile_task_batch(&feature_uid, &hash, &tasks)
            .unwrap();
        reopened
            .reconcile_task_batch(&feature_uid, &hash, &tasks)
            .unwrap();
        assert_eq!(reopened.list_tasks().unwrap().len(), 2);
        assert!(
            reopened
                .list_spec_sync_queue(&feature_uid, false)
                .unwrap()
                .is_empty()
        );
    }
}
