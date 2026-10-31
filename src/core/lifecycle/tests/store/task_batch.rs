use super::Fixture;

#[path = "task_batch/approved_feature.rs"]
mod feature_setup;
use feature_setup::approved_feature;

#[path = "task_batch/legacy.rs"]
mod legacy;
#[path = "task_batch/task.rs"]
mod task_record;
use crate::core::lifecycle::{FeaturePhase, LifecycleStore};
use task_record::task;

fn record_approved_spec_baseline(lifecycle: &LifecycleStore<'_>, feature_uid: &str) {
    let feature = lifecycle.load_feature(feature_uid).unwrap();
    let origins = lifecycle
        .list_spec_sync_queue(feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.source_event_uid)
        .collect();
    lifecycle
        .append_spec_revision(
            feature_uid,
            0,
            crate::core::lifecycle::SpecRevisionDraftV1 {
                no_change_reason: Some(
                    "Synthetic fixture records an explicit unchanged-specification result.".into(),
                ),
                content_hash: crate::core::lifecycle::product_spec::current_product_hash(
                    lifecycle.store,
                )
                .unwrap(),
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events: origins,
                decision_refs: feature.decision_refs,
                delta_approval_refs: Vec::new(),
                approved_target_hash: feature.approved_plan_sha256,
                implementation_head_sha: None,
                reconciliation_status:
                    crate::core::lifecycle::SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                deltas: Vec::new(),
            },
        )
        .unwrap();
}

#[test]
fn task_batch_retries_resume_partial_writes_and_reject_changed_bindings() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, plan_sha) = approved_feature(&lifecycle);
    let first = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let mut second = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    second.parent_task_uid = Some(first.task_uid.clone());
    let mut third = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    third.parent_task_uid = Some(first.task_uid.clone());
    third.dependency_task_uids = vec![second.task_uid.clone()];
    let mut first = first;
    first.subtask_uids = vec![second.task_uid.clone(), third.task_uid.clone()];

    let feature_revision_before = lifecycle.load_feature(&feature_uid).unwrap().revision;
    let error = lifecycle
        .reconcile_task_batch(&feature_uid, &plan_sha, std::slice::from_ref(&first))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("product specification is unavailable")
    );
    assert!(lifecycle.list_tasks().unwrap().is_empty());
    assert_eq!(
        lifecycle.load_feature(&feature_uid).unwrap().revision,
        feature_revision_before
    );
    crate::artifacts::migration::bootstrap_product(&fixture.0, "Lifecycle Task Batch").unwrap();
    let missing_baseline = lifecycle
        .reconcile_task_batch(&feature_uid, &plan_sha, std::slice::from_ref(&first))
        .unwrap_err();
    assert!(
        missing_baseline
            .to_string()
            .contains("no recorded lifecycle baseline")
    );
    assert!(lifecycle.list_tasks().unwrap().is_empty());
    record_approved_spec_baseline(&lifecycle, &feature_uid);
    lifecycle
        .reconcile_task_batch(&feature_uid, &plan_sha, std::slice::from_ref(&first))
        .unwrap();
    let reopened_store = fixture.store();
    let reopened = LifecycleStore::new(&reopened_store);
    reopened
        .reconcile_task_batch(
            &feature_uid,
            &plan_sha,
            &[first.clone(), second.clone(), third.clone()],
        )
        .unwrap();
    reopened
        .reconcile_task_batch(
            &feature_uid,
            &plan_sha,
            &[first.clone(), second.clone(), third.clone()],
        )
        .unwrap();

    let feature = reopened.load_feature(&feature_uid).unwrap();
    assert_eq!(feature.phase, FeaturePhase::Executing);
    assert_eq!(feature.task_uids.len(), 3);
    assert_eq!(reopened.replay_feature(&feature_uid).unwrap(), feature);
    assert_eq!(reopened.load_task(&first.task_uid).unwrap().revision, 1);
    // Production boundary: the board consumes this validated on-disk snapshot list.
    // The existing temp-store fixture proves persisted identity/revision checks beyond a pure unit test;
    // it needs no external services and adds no setup beyond this lifecycle store test.
    assert_eq!(reopened.list_tasks().unwrap().len(), 3);
    assert_eq!(
        reopened
            .task_for_planning_item(first.planning_item_uid.as_deref().unwrap())
            .unwrap()
            .unwrap()
            .task_uid,
        first.task_uid
    );
    assert_eq!(reopened.load_task(&second.task_uid).unwrap().revision, 1);
    assert_eq!(reopened.load_task(&third.task_uid).unwrap().revision, 1);
    assert_eq!(
        reopened.load_task(&first.task_uid).unwrap().subtask_uids,
        [second.task_uid.clone(), third.task_uid.clone()]
    );
    assert_eq!(
        reopened
            .load_task(&third.task_uid)
            .unwrap()
            .dependency_task_uids,
        [second.task_uid.clone()]
    );

    legacy::verify_fixture(&fixture, &reopened_store, &first.task_uid);

    let subtask_event = reopened
        .events_for_feature(&feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            matches!(
                event.event,
                crate::core::lifecycle::LifecycleEventKind::SubtaskCreated { .. }
            )
        })
        .unwrap();
    let event_path = fixture
        .0
        .join(format!("state/lifecycle_events/{}.json", subtask_event.uid));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    let fake_parent = uuid::Uuid::new_v4().hyphenated().to_string();
    event["event"]["parent_task_uid"] = fake_parent.clone().into();
    event["initialTask"]["parentTaskUid"] = fake_parent.into();
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();
    assert!(reopened.replay_feature(&feature_uid).is_err());

    let mut changed = first;
    changed.task_spec_sha256 = Some("e".repeat(64));
    assert!(
        reopened
            .reconcile_task_batch(&feature_uid, &plan_sha, &[changed, second, third])
            .is_err()
    );
}

/// Production boundary: task generation through the file-backed lifecycle
/// store must reject a changed rendered product spec before writing task or
/// graph records; a helper-only check cannot prove write ordering. Uses only
/// the synthetic temporary store (low setup cost, no external services).
#[test]
fn task_generation_rejects_spec_drift_before_persisting_tasks() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, plan_sha) = approved_feature(&lifecycle);
    crate::artifacts::migration::bootstrap_product(&fixture.0, "Spec Baseline A").unwrap();
    record_approved_spec_baseline(&lifecycle, &feature_uid);
    let generated = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );

    let documents = crate::artifacts::product_docs::load_documents(&store)
        .unwrap()
        .expect("recorded product baseline");
    let document = &documents[0];
    let relative_path = store
        .layout()
        .product_root()
        .join(&document.module.path)
        .strip_prefix(store.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(
            &[(
                relative_path,
                format!("{}\n\nA deliberate product change.\n", document.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
    let error = lifecycle
        .reconcile_task_batch(&feature_uid, &plan_sha, &[generated])
        .unwrap_err();
    assert!(error.to_string().contains("drifted"));
    assert!(lifecycle.list_tasks().unwrap().is_empty());
}

#[path = "task_batch/interrupted.rs"]
mod interrupted;
