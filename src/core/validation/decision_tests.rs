use super::*;
use crate::{
    domain::{
        ConfidenceLevel, DecisionBrief, DecisionConfidence, DecisionOption, DecisionRecommendation,
        ItemStatus,
    },
    harness::{TurnEnvelope, TurnItem},
};

fn brief() -> DecisionBrief {
    DecisionBrief {
        id: String::new(),
        question: "Which access behavior should the product use?".into(),
        why_now: "The release flow depends on the access rule.".into(),
        recommendation: Some(DecisionRecommendation {
            option_id: "time-limited".into(),
            rationale: "The repository documents a risk from long-lived sessions.".into(),
        }),
        confidence: Some(DecisionConfidence {
            level: ConfidenceLevel::Medium,
            explanation: "Current behavior is known; user tolerance is not.".into(),
        }),
        options: ["time-limited", "persistent"]
            .into_iter()
            .map(|id| DecisionOption {
                id: id.into(),
                label: format!("Use {id} access"),
                summary: format!("Apply the {id} access behavior."),
                benefits: vec!["Addresses the recorded access need.".into()],
                costs: vec!["Requires a policy change.".into()],
                risks: vec!["Changes the current user flow.".into()],
                consequences: vec!["The selected access behavior becomes product policy.".into()],
                reversibility: "The policy can be changed later.".into(),
            })
            .collect(),
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["Every signed-in user follows the chosen policy.".into()],
        reversibility: "The policy can be revisited after rollout.".into(),
        defer_consequence: "The feature cannot be finalized.".into(),
        evidence: vec!["src/auth/session.rs records current behavior.".into()],
        adr_assessment: Some(crate::domain::AdrAssessment {
            create: false,
            title: String::new(),
            rationale: "This choice can be changed without lasting architectural impact.".into(),
            revisit_when: vec![],
        }),
    }
}

fn state() -> PlannerState {
    let root = std::env::temp_dir().join(format!(
        "packet-decision-validation-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    PlannerState::load(&root).unwrap()
}

fn envelope() -> TurnEnvelope {
    TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Please choose the access behavior.".into()),
        change_summary: None,
        document_updates: None,
        updated_specification: None,
        open_items_added: Some(vec![TurnItem {
            priority: Some("High".into()),
            authority: Some("Human".into()),
            kind: Some("Question".into()),
            category: Some("Product".into()),
            assigned_to: Some("All".into()),
            question: Some("Should sessions expire automatically?".into()),
            reason: Some("The access policy affects security and client behavior.".into()),
            decision_brief: Some(brief()),
            ..Default::default()
        }]),
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        requested_action: None,
        interview: None,
        task_stories: None,
        task_outline: None,
    }
}

#[test]
fn human_decision_brief_binds_to_application_id_and_leaves_item_open() {
    let state = state();
    let result = validate(&envelope(), &state, &CurrentUser::new("Riley", vec![])).unwrap();
    let item = &result.added[0];
    assert_eq!(item.id, "CLR-001");
    assert_eq!(item.status, ItemStatus::Open);
    let brief = item.decision_brief.as_ref().unwrap();
    assert_eq!(brief.id, item.id);
    assert_eq!(brief.options.len(), 2);
    assert!(result.resolved.is_empty());
}

#[test]
fn decision_brief_cannot_recommend_an_unlisted_option() {
    let state = state();
    let mut envelope = envelope();
    envelope.open_items_added.as_mut().unwrap()[0]
        .decision_brief
        .as_mut()
        .unwrap()
        .recommendation
        .as_mut()
        .unwrap()
        .option_id = "invented".into();
    let errors = validate(&envelope, &state, &CurrentUser::new("Riley", vec![])).unwrap_err();
    assert!(errors.iter().any(|error| error.contains("listed option")));
}

#[test]
fn new_decision_brief_must_assess_durable_record_need() {
    let state = state();
    let mut envelope = envelope();
    envelope.open_items_added.as_mut().unwrap()[0]
        .decision_brief
        .as_mut()
        .unwrap()
        .adr_assessment = None;
    let errors = validate(&envelope, &state, &CurrentUser::new("Riley", vec![])).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.contains("durable decision record"))
    );
}
