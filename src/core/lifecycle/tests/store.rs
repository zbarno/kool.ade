use super::super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreError};

#[path = "store/completion.rs"]
mod completion;
#[path = "store/task_batch.rs"]
mod task_batch;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-lifecycle-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn store(&self) -> PlanningStore {
        PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &self.0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lifecycle_events_and_snapshots_commit_atomically_and_survive_reopen() {
    let fixture = Fixture::new();
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let create_event = uuid::Uuid::new_v4().hyphenated().to_string();
    let first_event = uuid::Uuid::new_v4().hyphenated().to_string();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let mut feature = FeatureGoal {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        original_intent: "persist an approved workflow".into(),
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
    lifecycle.create_feature(&feature, &create_event).unwrap();
    let packet = fixture.0.join(".koolade-packet/state");
    assert!(
        packet
            .join("lifecycle_features")
            .join(format!("{feature_uid}.json"))
            .is_file()
    );
    assert!(
        !packet
            .join("workflow")
            .join(format!("{feature_uid}.json"))
            .exists()
    );
    assert_eq!(lifecycle.load_feature(&feature_uid).unwrap().revision, 1);
    let changed = lifecycle
        .apply_feature_event(
            &feature_uid,
            1,
            &first_event,
            FeatureEvent::QuestionsRaised {
                questions: vec!["Confirm target repository".into()],
            },
        )
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert_eq!(
        lifecycle.load_feature(&feature_uid).unwrap().phase,
        FeaturePhase::AwaitingAnswers
    );

    let reopened_planning = fixture.store();
    let reopened = LifecycleStore::new(&reopened_planning);
    feature = reopened.load_feature(&feature_uid).unwrap();
    assert_eq!(feature.unresolved_questions, ["Confirm target repository"]);
    assert_eq!(reopened.events_for_feature(&feature_uid).unwrap().len(), 2);
    assert!(matches!(
        reopened.apply_feature_event(
            &feature_uid,
            1,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::AnswersResolved,
        ),
        Err(StoreError::StaleRecordRevision { .. })
    ));
    assert!(
        packet
            .join("lifecycle_events")
            .join(format!("{first_event}.json"))
            .is_file()
    );
}
#[test]
fn event_identity_cannot_be_replayed_to_overwrite_a_feature() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let mut feature = FeatureGoal {
        schema_version: 1,
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        original_intent: "track decisions".into(),
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
    feature.revision = 1;
    let event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .apply_feature_event(
            &feature.feature_uid,
            1,
            &event_uid,
            FeatureEvent::QuestionsRaised {
                questions: vec!["Need a decision".into()],
            },
        )
        .unwrap();
    assert!(matches!(
        lifecycle.apply_feature_event(
            &feature.feature_uid,
            2,
            &event_uid,
            FeatureEvent::AnswersResolved,
        ),
        Err(StoreError::StaleRecordRevision { .. })
    ));
}
