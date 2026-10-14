use super::*;

fn checklist() -> ReadinessChecklistV1 {
    ReadinessChecklistV1 {
        feature_uid: uuid::Uuid::from_u128(1).to_string(),
        plan_revision: 4,
        spec_revision: None,
        assessed_axes: MATERIAL_AXES.to_vec(),
        acceptance_tests: vec![AcceptanceTestTrace {
            requirement_id: "REQ-1".into(),
            test_ref: "tests/migration.rs".into(),
        }],
        evidence_refs: Vec::new(),
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: false,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    }
}

fn question() -> QuestionV1 {
    propose_questions(
        vec![QuestionProposal {
            feature_uid: uuid::Uuid::from_u128(1).to_string(),
            plan_revision: 4,
            decision_subject: "data owner".into(),
            decision_domain: "data ownership".into(),
            origin_requirement_ids: vec!["REQ-1".into()],
            category: QuestionCategory::Data,
            priority: QuestionPriority::Blocking,
            question: "Who owns it?".into(),
            reason: "Changes migration behavior".into(),
            options: vec![QuestionOption {
                id: "keep".into(),
                label: "Keep existing".into(),
                tradeoffs: "No migration".into(),
            }],
            recommended_option_id: Some("keep".into()),
            no_answer_impact: "Migration remains blocked".into(),
            evidence_refs: Vec::new(),
            assumption_if_deferred: None,
            depends_on_question_ids: Vec::new(),
        }],
        &[],
    )
    .unwrap()
    .remove(0)
}

#[test]
fn answered_question_recomputes_and_persists_readiness() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("readiness-answer-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&store);
    let mut question = question();
    api.save(&question, 0).unwrap();
    api.save_readiness(&checklist(), 0).unwrap();

    assert!(matches!(
        api.reassess_readiness(&question.feature_uid, question.plan_revision)
            .unwrap(),
        Some(ReadinessResult::NeedsWork { blockers, .. }) if blockers == [question.uid.clone()]
    ));
    let (saved, readiness_revision) = api.load_readiness(&question.feature_uid).unwrap().unwrap();
    assert!(!saved.ready);
    assert_eq!(saved.unresolved_blocker_ids, [question.uid.clone()]);

    let (loaded, record_revision) = api.load(&question.uid).unwrap();
    question = loaded;
    let question_revision = question.revision;
    let plan_revision = question.plan_revision;
    reconcile_answer(
        &mut question,
        question_revision,
        plan_revision,
        Some("The project data steward owns it."),
        None,
        "operator",
        5,
    )
    .unwrap();
    api.save(&question, record_revision).unwrap();
    assert_eq!(
        api.reassess_readiness(&question.feature_uid, question.plan_revision)
            .unwrap(),
        Some(ReadinessResult::Ready)
    );
    let (saved, next_revision) = api.load_readiness(&question.feature_uid).unwrap().unwrap();
    assert!(saved.ready);
    assert!(saved.unresolved_blocker_ids.is_empty());
    assert_eq!(next_revision, readiness_revision + 1);
    let reopened = PlanningStore::new(store.project_id, &root, StoreMode::ManagedLocal);
    assert_eq!(
        QuestionStore::new(&reopened)
            .load_readiness(&question.feature_uid)
            .unwrap()
            .unwrap(),
        (saved, next_revision)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn readiness_reassessment_rejects_a_different_plan_revision() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("readiness-stale-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&store);
    let question = question();
    api.save_readiness(&checklist(), 0).unwrap();
    assert!(
        api.reassess_readiness(&question.feature_uid, question.plan_revision + 1)
            .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn competing_readiness_updates_use_record_revision_cas() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("readiness-cas-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&store);
    let initial = checklist();
    api.save_readiness(&initial, 0).unwrap();
    let (mut first, first_revision) = api.load_readiness(&initial.feature_uid).unwrap().unwrap();
    let (mut second, second_revision) = api.load_readiness(&initial.feature_uid).unwrap().unwrap();
    first.revision += 1;
    first.ready = true;
    api.save_readiness(&first, first_revision).unwrap();
    second.revision += 1;
    assert!(api.save_readiness(&second, second_revision).is_err());
    let _ = std::fs::remove_dir_all(root);
}
