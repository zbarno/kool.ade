use super::super::*;

fn question() -> QuestionV1 {
    QuestionV1 {
        uid: uuid::Uuid::from_u128(1).to_string(),
        feature_uid: uuid::Uuid::from_u128(2).to_string(),
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
fn optional_deferral_is_attributed_and_does_not_answer_the_question() {
    let mut question = question();
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
    assert_eq!(question.status, QuestionStatus::Deferred);
    assert_eq!(question.revision, 1);
    assert_eq!(question.deferred_by.as_deref(), Some("operator"));
    assert_eq!(question.deferred_at_ms, Some(10));
    assert!(question.answer_text.is_none());
    assert!(question.answered_by.is_none());
    question.validate().unwrap();
}

#[test]
fn deferral_rejects_required_stale_or_incomplete_questions_without_mutation() {
    let mut required = question();
    required.priority = QuestionPriority::Blocking;
    assert!(defer_question(&mut required, 0, 3, details(), "operator", 10).is_err());

    let mut stale = question();
    assert!(defer_question(&mut stale, 1, 3, details(), "operator", 10).is_err());
    assert!(defer_question(&mut stale, 0, 4, details(), "operator", 10).is_err());
    let incomplete = DeferredAssumption {
        risk: String::new(),
        ..details()
    };
    assert!(defer_question(&mut stale, 0, 3, incomplete, "operator", 10).is_err());
    assert_eq!(stale.status, QuestionStatus::Open);
    assert_eq!(stale.revision, 0);
}
