use super::*;

fn option(id: &str) -> DecisionOption {
    DecisionOption {
        id: id.into(),
        label: format!("Path {id}"),
        summary: format!("Apply the {id} approach to the current issue."),
        benefits: vec![format!("The {id} approach addresses the recorded need.")],
        costs: vec![format!("The {id} approach requires implementation work.")],
        risks: vec![format!("The {id} approach has a follow-up risk.")],
        consequences: vec![format!("The product follows the {id} behavior.")],
        reversibility: "The setting can be revisited later.".into(),
    }
}

fn brief(options: Vec<DecisionOption>) -> DecisionBrief {
    DecisionBrief {
        id: String::new(),
        question: "Which behavior should the product use?".into(),
        why_now: "The unresolved choice affects this release's user flow.".into(),
        recommendation: options.first().map(|option| DecisionRecommendation {
            option_id: option.id.clone(),
            rationale: "Recorded evidence fits this option.".into(),
        }),
        confidence: Some(DecisionConfidence {
            level: ConfidenceLevel::Medium,
            explanation: "The repository shows current behavior but not user preference.".into(),
        }),
        options,
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["The choice changes the behavior users see.".into()],
        reversibility: "The policy can be changed later.".into(),
        defer_consequence: "The affected work cannot be finalized.".into(),
        evidence: vec!["src/current_behavior.rs records the existing flow.".into()],
        adr_assessment: Some(AdrAssessment {
            create: false,
            title: String::new(),
            rationale: "This is a reversible product choice, not a durable architecture decision."
                .into(),
            revisit_when: vec![],
        }),
    }
}

#[test]
fn brief_binds_to_item_identity_and_supports_issue_specific_option_counts() {
    let choices = (1..=10)
        .map(|index| option(&format!("path-{index}")))
        .collect();
    let mut decision = brief(choices);
    assert!(decision.bind_to_item("CLR-042").is_ok());
    assert_eq!(decision.id, "CLR-042");
    assert_eq!(decision.options.len(), 10);

    let serialized = serde_json::to_string(&decision).unwrap();
    let restored: DecisionBrief = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored, decision);
}

#[test]
fn brief_rejects_an_invented_recommendation_and_one_option_menu() {
    let mut decision = brief(vec![option("path-a"), option("path-b")]);
    decision.recommendation.as_mut().unwrap().option_id = "unlisted".into();
    assert!(decision.validate().unwrap_err().contains("listed option"));

    let single = brief(vec![option("only-path")]);
    assert!(single.validate().unwrap_err().contains("zero or multiple"));
}

#[test]
fn material_record_needs_real_alternatives_and_a_revisit_condition() {
    let mut decision = brief(vec![option("path-a"), option("path-b")]);
    let assessment = decision.adr_assessment.as_mut().unwrap();
    assessment.create = true;
    assessment.title = "Choose a durable storage policy".into();
    assessment.revisit_when.clear();
    assert!(
        decision
            .validate()
            .unwrap_err()
            .contains("revisit condition")
    );

    decision.adr_assessment.as_mut().unwrap().revisit_when =
        vec!["Usage changes beyond the planned retention window.".into()];
    decision.options.clear();
    decision.recommendation = None;
    assert!(
        decision
            .validate()
            .unwrap_err()
            .contains("two alternatives")
    );
}
