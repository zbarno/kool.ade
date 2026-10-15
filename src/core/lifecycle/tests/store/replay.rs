use super::Fixture;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{FeatureEvent, FeatureGoal, FeaturePhase, LifecycleStore};
use std::sync::{Arc, Barrier};

#[test]
fn replay_restores_a_missing_feature_snapshot_and_preserves_corrupt_records() {
    let fixture = Fixture::new();
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let mut feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "recover a missing lifecycle projection".into(),
        draft: String::new(),
        scope: Vec::new(),
        non_goals: Vec::new(),
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: None,
        plan_snapshot_sha256: None,
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    };
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            1,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::QuestionsRaised {
                questions: vec!["Which user owns the decision?".into()],
            },
        )
        .unwrap();
    let snapshot_path = fixture.0.join(format!(
        ".koolade-packet/state/lifecycle_features/{feature_uid}.json"
    ));
    std::fs::remove_file(&snapshot_path).unwrap();
    let root = fixture.0.clone();
    let barrier = Arc::new(Barrier::new(3));
    let recoveries = (0..2)
        .map(|_| {
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            let feature_uid = feature_uid.clone();
            std::thread::spawn(move || {
                let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
                let lifecycle = LifecycleStore::new(&store);
                barrier.wait();
                lifecycle.replay_feature(&feature_uid)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    for recovery in recoveries {
        assert_eq!(recovery.join().unwrap().unwrap(), feature);
    }
    assert_eq!(lifecycle.replay_feature(&feature_uid).unwrap(), feature);
    assert_eq!(lifecycle.load_feature(&feature_uid).unwrap(), feature);

    std::fs::write(&snapshot_path, b"not valid json").unwrap();
    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert_eq!(std::fs::read(&snapshot_path).unwrap(), b"not valid json");
}

#[test]
fn replay_rejects_a_v3_feature_event_without_revision_metadata() {
    let fixture = Fixture::new();
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let create_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let transition_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "detect an incomplete event history".into(),
        draft: String::new(),
        scope: Vec::new(),
        non_goals: Vec::new(),
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: None,
        plan_snapshot_sha256: None,
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    };
    lifecycle.create_feature(&feature, &create_uid).unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            1,
            &transition_uid,
            FeatureEvent::QuestionsRaised {
                questions: vec!["Identify the approver".into()],
            },
        )
        .unwrap();

    let path = fixture.0.join(format!(
        ".koolade-packet/state/lifecycle_events/{transition_uid}.json"
    ));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    event
        .as_object_mut()
        .unwrap()
        .remove("featureRevisionAfter");
    std::fs::write(&path, serde_json::to_vec(&event).unwrap()).unwrap();

    assert!(lifecycle.replay_feature(&feature_uid).is_err());
}

#[test]
fn legacy_creation_history_remains_readable_but_is_not_replayable() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let create_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "preserve a version one snapshot".into(),
        draft: String::new(),
        scope: Vec::new(),
        non_goals: Vec::new(),
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: None,
        plan_snapshot_sha256: None,
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    };
    lifecycle.create_feature(&feature, &create_uid).unwrap();
    let path = fixture.0.join(format!(
        ".koolade-packet/state/lifecycle_events/{create_uid}.json"
    ));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let object = event.as_object_mut().unwrap();
    object.insert("schemaVersion".into(), serde_json::Value::from(1));
    object.remove("featureRevisionAfter");
    object.remove("initialFeature");
    std::fs::write(&path, serde_json::to_vec(&event).unwrap()).unwrap();

    assert_eq!(lifecycle.load_feature(&feature_uid).unwrap().revision, 1);
    assert!(lifecycle.replay_feature(&feature_uid).is_err());
}
