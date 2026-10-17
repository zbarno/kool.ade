use super::*;

fn open_question(category: QuestionCategory) -> QuestionV1 {
    super::super::propose_questions(
        vec![super::super::QuestionProposal {
            feature_uid: uuid::Uuid::from_u128(1).to_string(),
            plan_revision: 3,
            decision_subject: "API response envelope".into(),
            decision_domain: "integration contract".into(),
            origin_requirement_ids: vec!["REQ-1".into()],
            category,
            priority: super::super::QuestionPriority::Important,
            question: "Which envelope matches the established interface?".into(),
            reason: "The consuming integration must parse the response.".into(),
            options: vec![
                super::super::QuestionOption {
                    id: "existing".into(),
                    label: "Existing envelope".into(),
                    tradeoffs: "Matches current clients.".into(),
                },
                super::super::QuestionOption {
                    id: "new".into(),
                    label: "New envelope".into(),
                    tradeoffs: "Requires client changes.".into(),
                },
            ],
            recommended_option_id: Some("existing".into()),
            no_answer_impact: "The integration contract stays open.".into(),
            evidence_refs: vec!["feature:1".into()],
            assumption_if_deferred: None,
            depends_on_question_ids: vec![],
        }],
        &[],
    )
    .unwrap()
    .remove(0)
}

#[test]
fn evidence_resolution_cannot_answer_user_intent_or_policy_questions() {
    for category in [
        QuestionCategory::Goal,
        QuestionCategory::Scope,
        QuestionCategory::Ux,
        QuestionCategory::Data,
        QuestionCategory::Security,
        QuestionCategory::Privacy,
        QuestionCategory::FailureMode,
    ] {
        let mut question = open_question(category);
        assert!(
            resolve_question_with_evidence(
                &mut question,
                super::super::EvidenceResolution {
                    expected_revision: 0,
                    current_plan_revision: 3,
                    decision: "Use the established interface",
                    rationale: "The repository overview shows the existing convention.",
                    selected_option: Some("existing"),
                    evidence_refs: &["repository-overview:1".into()],
                    now_ms: 1,
                },
            )
            .is_err()
        );
        assert_eq!(question.status, QuestionStatus::Open);
    }
}
