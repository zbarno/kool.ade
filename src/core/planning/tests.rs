use super::*;

#[path = "tests/readiness_store.rs"]
mod readiness_store;

fn proposal(subject: &str, wording: &str) -> QuestionProposal {
    QuestionProposal {
        feature_uid: uuid::Uuid::from_u128(1).to_string(),
        plan_revision: 4,
        decision_subject: subject.into(),
        decision_domain: "data ownership".into(),
        origin_requirement_ids: vec!["REQ-1".into()],
        category: QuestionCategory::Data,
        priority: QuestionPriority::Blocking,
        question: wording.into(),
        reason: "Changes migration behavior".into(),
        options: vec![QuestionOption {
            id: "keep".into(),
            label: "Keep existing".into(),
            tradeoffs: "No migration".into(),
        }],
        recommended_option_id: Some("keep".into()),
        no_answer_impact: "Migration remains blocked".into(),
        evidence_refs: vec![],
        assumption_if_deferred: None,
        depends_on_question_ids: vec![],
    }
}

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
        evidence_refs: vec![],
        unresolved_blocker_ids: vec![],
        blocker_notes: vec![],
        ready: false,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    }
}

#[test]
fn stylistic_duplicates_collapse_and_existing_questions_are_idempotent() {
    let created = propose_questions(
        vec![
            proposal("Who owns customer data?", "Who owns the customer data?"),
            proposal("who owns customer data", "Who owns this data?"),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(created.len(), 1);
    assert!(
        propose_questions(
            vec![proposal("WHO owns customer data", "Who owns data?")],
            &created
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn duplicate_wording_is_selected_deterministically_across_response_order() {
    let first = proposal("data owner", "Who owns customer data?");
    let second = proposal("DATA OWNER", "Which team owns customer data?");
    let forward = propose_questions(vec![first.clone(), second.clone()], &[]).unwrap();
    let reverse = propose_questions(vec![second, first], &[]).unwrap();
    assert_eq!(forward, reverse);
}

#[test]
fn stale_answers_are_rejected_and_declines_are_valid_answers() {
    let mut q = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    assert!(reconcile_answer(&mut q, 1, 4, Some("answer"), None, "operator", 5).is_err());
    assert!(reconcile_answer(&mut q, 0, 5, Some("answer"), None, "operator", 5).is_err());
    reconcile_answer(
        &mut q,
        0,
        4,
        Some("I decline to choose"),
        None,
        "operator",
        5,
    )
    .unwrap();
    assert_eq!(q.status, QuestionStatus::Answered);
}

#[test]
fn readiness_requires_blockers_and_acceptance_trace_but_allows_deferred_optional() {
    let mut q = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    assert!(matches!(
        assess_readiness(&[q.clone()], &checklist(), &["REQ-1".into()]),
        ReadinessResult::NeedsWork { .. }
    ));
    q.priority = QuestionPriority::Optional;
    q.deferred = Some(DeferredAssumption {
        assumption: "Use project owner".into(),
        owner: "team".into(),
        risk: "May change".into(),
        verification_test: "ownership test".into(),
        follow_up_task: "review later".into(),
    });
    assert_eq!(
        assess_readiness(&[q], &checklist(), &["REQ-1".into()]),
        ReadinessResult::Ready
    );
}

#[test]
fn readiness_requires_at_least_one_traceable_acceptance_test() {
    let mut state = checklist();
    state.acceptance_tests.clear();
    assert!(matches!(
        assess_readiness(&[], &state, &[]),
        ReadinessResult::NeedsWork {
            missing_acceptance_tests: true,
            ..
        }
    ));
    state.ready = true;
    assert!(state.validate().is_err());
}

#[test]
fn planner_blocker_notes_keep_readiness_unready() {
    let mut state = checklist();
    state
        .blocker_notes
        .push("Retention policy is undecided".into());
    assert!(matches!(
        assess_readiness(&[], &state, &[]),
        ReadinessResult::NeedsWork { blockers, .. }
            if blockers == vec!["Retention policy is undecided"]
    ));
}

#[test]
fn readiness_rejects_question_records_from_another_plan_revision() {
    let mut question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    question.plan_revision += 1;
    assert!(matches!(
        assess_readiness(&[question], &checklist(), &["REQ-1".into()]),
        ReadinessResult::InvalidEvidence { .. }
    ));
}

#[test]
fn unanswered_draft_blocker_prevents_readiness() {
    let question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    let mut question = question;
    question.status = QuestionStatus::Draft;
    assert!(matches!(
        assess_readiness(&[question], &checklist(), &["REQ-1".into()]),
        ReadinessResult::NeedsWork { blockers, .. } if blockers.len() == 1
    ));
}

#[test]
fn declining_a_required_question_is_persistable_but_not_a_decision() {
    let mut question = propose_questions(vec![proposal("retention", "How long?")], &[])
        .unwrap()
        .remove(0);
    decline_question(&mut question, 0, 4, "operator", 1_700_000_000_001).unwrap();
    assert_eq!(question.status, QuestionStatus::Declined);
    assert!(question.answer_text.is_none());
    assert!(matches!(
        assess_readiness(&[question], &checklist(), &["REQ-1".into()]),
        ReadinessResult::NeedsWork { blockers, .. } if blockers.len() == 1
    ));
}

#[test]
fn repeated_identical_passes_eventually_stall_without_question_count_limit() {
    let mut state = checklist();
    assert!(!observe_progress(&mut state, &[], 2));
    assert!(!observe_progress(&mut state, &[], 2));
    assert!(observe_progress(&mut state, &[], 2));
    assert!(matches!(
        assess_readiness(&[], &state, &["REQ-1".into()]),
        ReadinessResult::PlanningStalled { .. }
    ));
}

#[test]
fn question_store_round_trips_and_rejects_stale_record_revision() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("planning-questions-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let api = QuestionStore::new(&store);
    let mut question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    api.save(&question, 0).unwrap();
    let (created, _) = store
        .read_record::<serde_json::Value>(&format!("state/items/{}.json", question.uid))
        .unwrap();
    let created_at_ms = created["createdAtMs"].as_u64().unwrap();
    assert!(created_at_ms > 0);
    let (loaded, revision) = api.load(&question.uid).unwrap();
    assert_eq!(loaded, question);
    assert_eq!(revision, 1);
    question.revision = 1;
    api.save(&question, revision).unwrap();
    let (updated, _) = store
        .read_record::<serde_json::Value>(&format!("state/items/{}.json", question.uid))
        .unwrap();
    assert_eq!(updated["createdAtMs"].as_u64(), Some(created_at_ms));
    assert!(updated["updatedAtMs"].as_u64().unwrap() > 0);
    assert!(api.save(&question, revision).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_question_records_load_and_migrate_to_item_paths_on_write() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!(
        "planning-question-migration-{}",
        uuid::Uuid::new_v4()
    ));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let mut question = propose_questions(vec![proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    let created_at = 1_700_000_000_000_u64;
    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "uid": question.uid.clone(),
        "createdAtMs": created_at,
        "updatedAtMs": created_at,
        "revision": 1,
        "data": question.clone(),
    });
    let old_path = format!("state/questions/{}.json", question.uid);
    store.save_record(&old_path, &legacy, 0).unwrap();
    let old_bytes = store.read(&old_path).unwrap();

    let api = QuestionStore::new(&store);
    let (loaded, outer_revision) = api.load(&question.uid).unwrap();
    assert_eq!(loaded, question);
    assert_eq!(outer_revision, 1);
    reconcile_answer(
        &mut question,
        0,
        4,
        Some("Use the data steward <script>alert(1)</script>"),
        None,
        "operator",
        created_at + 1,
    )
    .unwrap();
    api.save(&question, outer_revision).unwrap();

    let (migrated, revision) = api.load(&question.uid).unwrap();
    assert_eq!(migrated, question);
    assert_eq!(
        revision, 1,
        "migrated canonical record starts at outer revision one"
    );
    assert_eq!(
        store.read(&old_path).unwrap(),
        old_bytes,
        "legacy record is retained unchanged"
    );
    let markdown = String::from_utf8(
        store
            .read(format!("planning/items/{}.md", question.uid))
            .unwrap(),
    )
    .unwrap();
    assert!(markdown.contains("## Recorded decision"));
    assert!(markdown.contains("Use the data steward &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!markdown.contains("<script>"));
    assert!(markdown.contains("Answered by: operator"));
    let (open, resolved, _) = crate::artifacts::items_io::load_store(&store).unwrap();
    assert!(
        open.is_empty() && resolved.is_empty(),
        "typed clarifications are not code-running item cards"
    );
    let _ = std::fs::remove_dir_all(root);
}
