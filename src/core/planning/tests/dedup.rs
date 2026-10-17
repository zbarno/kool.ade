use super::*;
use crate::core::planning::QuestionCategory;

fn proposal(requirements: Vec<&str>) -> QuestionProposal {
    QuestionProposal {
        feature_uid: uuid::Uuid::from_u128(21).to_string(),
        plan_revision: 4,
        decision_subject: "data ownership".into(),
        decision_domain: "ownership".into(),
        origin_requirement_ids: requirements.into_iter().map(str::to_owned).collect(),
        category: QuestionCategory::Data,
        priority: QuestionPriority::Optional,
        question: "Who owns the data?".into(),
        reason: "Ownership affects access.".into(),
        options: Vec::new(),
        recommended_option_id: None,
        no_answer_impact: "The owner remains an assumption.".into(),
        evidence_refs: Vec::new(),
        assumption_if_deferred: None,
        depends_on_question_ids: Vec::new(),
    }
}

#[test]
fn normalized_requirement_alias_keeps_the_canonical_trace_id() {
    let created = propose_questions(vec![proposal(vec![" req 1", "REQ-1"])], &[]).unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].origin_requirement_ids, vec!["REQ-1".to_owned()]);
}

#[test]
fn noncanonical_or_empty_requirement_id_without_canonical_match_is_rejected() {
    for value in ["req 1", "REQ-"] {
        let error = propose_questions(vec![proposal(vec![value])], &[]).unwrap_err();
        assert!(error.contains("canonical REQ- identifiers"));
    }
}

#[test]
fn related_paraphrases_merge_by_normalized_decision_key() {
    let mut first = proposal(vec!["REQ-1"]);
    first.question = "Who owns the data?".into();
    first.evidence_refs = vec!["feature:one".into()];
    let mut second = proposal(vec!["REQ-1"]);
    second.decision_subject = "DATA OWNERSHIP".into();
    second.question = "Which person or team is responsible for this data?".into();
    second.priority = QuestionPriority::Blocking;
    second.evidence_refs = vec!["feature:two".into()];

    let merged = propose_questions(vec![first, second], &[]).unwrap();

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].priority, QuestionPriority::Blocking);
    assert_eq!(merged[0].origin_requirement_ids, ["REQ-1"]);
    assert_eq!(merged[0].evidence_refs, ["feature:one", "feature:two"]);
}

#[test]
fn superseding_an_answered_question_preserves_its_recorded_decision() {
    let mut question = propose_questions(vec![proposal(vec!["REQ-1"])], &[])
        .unwrap()
        .remove(0);
    reconcile_answer(
        &mut question,
        0,
        4,
        Some("Keep the existing owner."),
        None,
        "operator",
        10,
    )
    .unwrap();

    supersede_question(&mut question, 1, 5).unwrap();

    assert_eq!(question.status, QuestionStatus::Superseded);
    assert_eq!(
        question.answer_text.as_deref(),
        Some("Keep the existing owner.")
    );
    assert_eq!(question.answered_by.as_deref(), Some("operator"));
    assert_eq!(question.revision, 2);
    question.validate().unwrap();
}
