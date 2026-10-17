use super::*;

#[path = "tests/readiness_progress.rs"]
mod readiness_progress;
#[path = "tests/readiness_store.rs"]
mod readiness_store;
#[path = "tests/retirement.rs"]
mod retirement;
#[path = "tests/store_records.rs"]
mod store_records;
#[path = "tests/traceability.rs"]
mod traceability;

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
        required_requirement_ids: Some(vec!["REQ-1".into()]),
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
    defer_question(
        &mut q,
        0,
        4,
        DeferredAssumption {
            assumption: "Use project owner".into(),
            owner: "team".into(),
            risk: "May change".into(),
            verification_test: "ownership test".into(),
            follow_up_task: "review later".into(),
        },
        "operator",
        5,
    )
    .unwrap();
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
        assess_readiness(&[], &state, &["REQ-1".into()]),
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
        assess_readiness(&[], &state, &["REQ-1".into()]),
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
