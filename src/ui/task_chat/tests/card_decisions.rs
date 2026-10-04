use super::card_support::*;
use crate::domain::Authority;
use crate::domain::item::{ItemKind, OpenItem, Priority};
use crate::ui::theme;
#[test]
fn human_decision_card_shows_consequences_recommendation_and_freeform_reply() {
    let mut probe = CardProbe::answer_card("Choose a retention period", "");
    probe.items[0].authority = Authority::Human;
    probe.items[0].decision_brief = Some(crate::domain::DecisionBrief {
        id: "CLR-001".into(),
        question: "Choose a retention period".into(),
        why_now: "The launch must settle how long saved information stays available.".into(),
        recommendation: Some(crate::domain::DecisionRecommendation {
            option_id: "short".into(),
            rationale: "A 30-day limit reduces how much old information remains available if an account is misused.".into(),
        }),
        confidence: Some(crate::domain::DecisionConfidence {
            level: crate::domain::ConfidenceLevel::Medium,
            explanation: "We know records are stored, but the right period depends on your rules."
                .into(),
        }),
        options: vec![
            crate::domain::DecisionOption {
                id: "short".into(),
                label: "Keep records for 30 days".into(),
                summary: "Remove older records automatically.".into(),
                benefits: vec![],
                costs: vec![],
                risks: vec![],
                consequences: vec!["Information older than 30 days is removed automatically.".into()],
                reversibility: "You can change how long information is kept.".into(),
            },
            crate::domain::DecisionOption {
                id: "all".into(),
                label: "Keep records until deleted".into(),
                summary: "Users remove records themselves.".into(),
                benefits: vec![],
                costs: vec![],
                risks: vec![],
                consequences: vec!["Information remains available until you delete it.".into()],
                reversibility: "You can add an automatic time limit later.".into(),
            },
        ],
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec![],
        reversibility: "You can change how long information is kept.".into(),
        defer_consequence: "The launch cannot proceed until a retention rule is chosen.".into(),
        evidence: vec![],
        adr_assessment: None,
    });
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut output = card_frame(&ctx, &mut probe, false, Vec::new());
    for content in [
        "The launch must settle how long saved information stays available.",
        "Kool.ad/e recommends",
        "Keep records for 30 days",
        "A 30-day limit reduces how much old information remains available if an account is misused.",
        "Information older than 30 days is removed automatically.",
        "Information remains available until you delete it.",
        "Confidence: Medium",
        "We know records are stored, but the right period depends on your rules.",
        "Can this change later? You can change how long information is kept.",
        "If you wait: The launch cannot proceed until a retention rule is chosen.",
        "Send answer",
    ] {
        assert!(card_has_text(&output, content), "missing {content}");
    }
    let choice = card_point(&output, "Keep records until deleted").unwrap();
    card_frame(&ctx, &mut probe, false, press_events(choice))
        .textures_delta
        .clear();
    card_frame(&ctx, &mut probe, false, release_events(choice))
        .textures_delta
        .clear();
    assert_eq!(probe.sent, 0, "a selection remains an explicit draft");
    assert!(probe.draft.contains("I choose option (all)"));
    output.textures_delta.clear();
}

#[test]
fn dependent_human_item_shows_its_prerequisite_without_an_answer_control() {
    let mut probe = CardProbe::answer_card("Choose recovery behavior", "");
    let parent = OpenItem::new(
        "CLR-010".into(),
        Priority::High,
        ItemKind::Question,
        "General".into(),
        None,
        "Choose an account model".into(),
        String::new(),
    );
    let mut dependent = OpenItem::new(
        "CLR-001".into(),
        Priority::High,
        ItemKind::Question,
        "General".into(),
        None,
        "Choose recovery behavior".into(),
        String::new(),
    );
    dependent.blocked_by = vec![parent.id.clone()];
    probe.items = vec![parent, dependent];
    probe.messages.clear();
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut output = card_frame(&ctx, &mut probe, false, Vec::new());
    assert!(card_has_text(&output, "Waiting for a prerequisite"));
    assert!(card_has_text(&output, "Resolve CLR-010 first."));
    assert!(!card_has_text(&output, "Your answer needed"));
    assert!(!card_has_text(&output, "Send answer"));
    output.textures_delta.clear();
}
