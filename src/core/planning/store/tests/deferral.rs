use super::*;
use crate::core::planning::{
    DeferredAssumption, QuestionCategory, QuestionPriority, QuestionStatus, defer_question,
};

fn optional_question() -> QuestionV1 {
    QuestionV1 {
        uid: uuid::Uuid::from_u128(11).to_string(),
        feature_uid: uuid::Uuid::from_u128(12).to_string(),
        plan_revision: 3,
        decision_subject: "data ownership".into(),
        decision_domain: "ownership".into(),
        origin_requirement_ids: vec!["REQ-1".into()],
        category: QuestionCategory::Data,
        priority: QuestionPriority::Optional,
        question: "Who owns customer data?".into(),
        reason: "Ownership affects the handoff.".into(),
        options: Vec::new(),
        recommended_option_id: None,
        no_answer_impact: "Ownership stays assumed.".into(),
        evidence_refs: Vec::new(),
        assumption_if_deferred: Some("Use the workspace owner".into()),
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: QuestionStatus::Open,
        answer_text: None,
        evidence_resolution: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    }
}

fn details() -> DeferredAssumption {
    DeferredAssumption {
        assumption: "Use the workspace owner".into(),
        owner: "planning team".into(),
        risk: "Owner could change".into(),
        verification_test: "Check the assigned owner".into(),
        follow_up_task: "Review before release".into(),
    }
}

#[test]
// Production boundary: deferral metadata and its human-readable record must persist
// atomically across a planning-store reopen. A unit test cannot prove disk serialization.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, using one synthetic optional question.
fn deferral_persists_state_and_markdown_atomically_and_reopens() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("question-deferral-{}", uuid::Uuid::new_v4()));
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&planning);
    let question = optional_question();
    api.save(&question, 0).unwrap();
    let (mut question, record_revision) = api.load(&question.uid).unwrap();
    let plan_revision = question.plan_revision;
    let expected_revision = question.revision;
    defer_question(
        &mut question,
        expected_revision,
        plan_revision,
        details(),
        "operator",
        10,
    )
    .unwrap();
    let expected_store_revision = planning.revision().unwrap();
    api.save_with_planning_revision(&question, record_revision, &expected_store_revision)
        .unwrap();

    let reopened = PlanningStore::new(planning.project_id, &root, StoreMode::ManagedLocal);
    let reopened_api = QuestionStore::new(&reopened);
    let (saved, _) = reopened_api.load(&question.uid).unwrap();
    assert_eq!(saved, question);
    let markdown = reopened
        .read(format!("planning/items/{}.md", saved.uid))
        .unwrap();
    assert!(
        String::from_utf8(markdown)
            .unwrap()
            .contains("## Deferred assumption")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: the planning store must reject a stale project revision without
// partially updating either state JSON or Markdown. A unit test cannot prove this CAS.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, using one synthetic optional question.
fn deferral_rejects_a_changed_planning_revision_without_partial_write() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root =
        std::env::temp_dir().join(format!("question-deferral-fence-{}", uuid::Uuid::new_v4()));
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&planning);
    let question = optional_question();
    api.save(&question, 0).unwrap();
    let (mut question, record_revision) = api.load(&question.uid).unwrap();
    let expected_store_revision = planning.revision().unwrap();
    let state_path = format!("state/items/{}.json", question.uid);
    let markdown_path = format!("planning/items/{}.md", question.uid);
    let state_before = planning.read(&state_path).unwrap();
    let markdown_before = planning.read(&markdown_path).unwrap();
    planning
        .transaction(
            &[("state/concurrent.json".into(), b"updated".to_vec())],
            None,
        )
        .unwrap();
    let expected_revision = question.revision;
    let expected_plan_revision = question.plan_revision;
    defer_question(
        &mut question,
        expected_revision,
        expected_plan_revision,
        details(),
        "operator",
        10,
    )
    .unwrap();
    assert!(
        api.save_with_planning_revision(&question, record_revision, &expected_store_revision)
            .is_err()
    );
    assert_eq!(planning.read(&state_path).unwrap(), state_before);
    assert_eq!(planning.read(&markdown_path).unwrap(), markdown_before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: a planner batch must leave every question and readiness
// record unchanged when its shared-store revision fence loses a race. Unit tests of
// individual record encoders cannot establish this multi-record transaction contract.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, using two synthetic clarification records.
fn planner_batch_rejects_a_stale_store_revision_without_partial_writes() {
    use crate::artifacts::planning_store::StoreMode;
    use crate::core::planning::ReadinessChecklistV1;
    let root = std::env::temp_dir().join(format!("question-batch-{}", uuid::Uuid::new_v4()));
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&planning);
    let first = optional_question();
    api.save(&first, 0).unwrap();
    let (mut update, first_record_revision) = api.load(&first.uid).unwrap();
    let update_revision = update.revision;
    let update_plan_revision = update.plan_revision;
    defer_question(
        &mut update,
        update_revision,
        update_plan_revision,
        details(),
        "operator",
        10,
    )
    .unwrap();
    let second = QuestionV1 {
        uid: uuid::Uuid::from_u128(13).to_string(),
        ..optional_question()
    };
    let checklist = ReadinessChecklistV1 {
        feature_uid: first.feature_uid.clone(),
        plan_revision: first.plan_revision,
        spec_revision: Some("a".repeat(64)),
        required_requirement_ids: Some(vec!["REQ-1".into()]),
        assessed_axes: crate::core::planning::MATERIAL_AXES.to_vec(),
        acceptance_tests: vec![crate::core::planning::AcceptanceTestTrace {
            requirement_id: "REQ-1".into(),
            test_ref: "tests/owner.rs".into(),
        }],
        evidence_refs: vec![],
        unresolved_blocker_ids: vec![],
        blocker_notes: vec![],
        ready: false,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    };
    let stale_revision = planning.revision().unwrap();
    planning
        .transaction(
            &[("state/concurrent.json".into(), b"changed".to_vec())],
            None,
        )
        .unwrap();

    assert!(
        api.save_batch_with_readiness(
            &[(update, first_record_revision), (second.clone(), 0)],
            &checklist,
            0,
            &stale_revision,
        )
        .is_err()
    );
    assert_eq!(api.load(&first.uid).unwrap().0, first);
    assert!(api.load(&second.uid).is_err());
    assert!(api.load_readiness(&first.feature_uid).unwrap().is_none());
    let _ = std::fs::remove_dir_all(root);
}
