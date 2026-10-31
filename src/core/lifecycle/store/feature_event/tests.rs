use super::{commit_after_read, read_for_apply};
use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::lifecycle::{FeatureEvent, FeatureGoal, FeaturePhase, LifecycleStore},
};
use std::sync::{Arc, Barrier};

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-lifecycle-read-commit-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn store(&self) -> PlanningStore {
        PlanningStore::new(
            uuid::Uuid::new_v4(),
            &self.0,
            crate::artifacts::planning_store::StoreMode::ManagedLocal,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_feature() -> FeatureGoal {
    FeatureGoal {
        schema_version: 1,
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        original_intent: "fence the lifecycle read-commit window".into(),
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
    }
}

#[test]
fn contenders_that_read_one_revision_can_commit_only_one_transition() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let feature = new_feature();
    let feature_uid = feature.feature_uid.clone();
    let lifecycle = LifecycleStore::new(&store);
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();

    let (left_path, left_snapshot) = read_for_apply(&store, &feature_uid, 1).unwrap();
    let (right_path, right_snapshot) = read_for_apply(&store, &feature_uid, 1).unwrap();
    assert_eq!(left_snapshot.revision, 1);
    assert_eq!(right_snapshot.revision, 1);
    assert_eq!(left_path, right_path);

    let barrier = Arc::new(Barrier::new(2));
    let left_store = store.clone();
    let left_uid = feature_uid.clone();
    let left_barrier = Arc::clone(&barrier);
    let left = std::thread::spawn(move || {
        left_barrier.wait();
        commit_after_read(
            &left_store,
            &left_uid,
            &left_path,
            1,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            FeatureEvent::QuestionsRaised {
                questions: vec!["Select an integration target".into()],
            },
            left_snapshot,
        )
    });

    let right_store = store.clone();
    let right_uid = feature_uid.clone();
    let right_barrier = Arc::clone(&barrier);
    let right = std::thread::spawn(move || {
        right_barrier.wait();
        commit_after_read(
            &right_store,
            &right_uid,
            &right_path,
            1,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            FeatureEvent::Cancel,
            right_snapshot,
        )
    });

    let outcomes = [left.join().unwrap(), right.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(StoreError::StaleRecordRevision { .. })))
            .count(),
        1,
        "the transaction must reject the contender holding the stale read"
    );

    let reopened = fixture.store();
    let lifecycle = LifecycleStore::new(&reopened);
    let final_feature = lifecycle.load_feature(&feature_uid).unwrap();
    assert_eq!(final_feature.revision, 2);
    assert!(matches!(
        final_feature.phase,
        FeaturePhase::AwaitingAnswers | FeaturePhase::Cancelled
    ));
    assert_eq!(lifecycle.events_for_feature(&feature_uid).unwrap().len(), 2);
}
