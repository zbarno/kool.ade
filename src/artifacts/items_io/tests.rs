use super::*;
use crate::domain::{
    AdrAssessment, Authority, ConfidenceLevel, DecisionBrief, DecisionConfidence, DecisionOption,
    DecisionRecommendation, ItemKind, Priority,
};

fn decision_brief(id: &str) -> DecisionBrief {
    DecisionBrief {
        id: id.into(),
        question: "Should API tokens expire automatically?".into(),
        why_now: "Token lifetime affects the security contract and client refresh behavior.".into(),
        recommendation: Some(DecisionRecommendation {
            option_id: "expiring".into(),
            rationale: "The documented threat model includes stolen active tokens.".into(),
        }),
        confidence: Some(DecisionConfidence {
            level: ConfidenceLevel::Medium,
            explanation: "Current token behavior is documented; user impact is not.".into(),
        }),
        options: vec![
            DecisionOption {
                id: "expiring".into(),
                label: "Expire tokens".into(),
                summary: "Require clients to refresh tokens periodically.".into(),
                benefits: vec!["Limits the life of a stolen token.".into()],
                costs: vec!["Clients must refresh before expiration.".into()],
                risks: vec!["A failed refresh can interrupt active work.".into()],
                consequences: vec!["Token lifetime becomes a security setting.".into()],
                reversibility: "The lifetime can be changed later.".into(),
            },
            DecisionOption {
                id: "persistent".into(),
                label: "Keep tokens persistent".into(),
                summary: "Tokens remain valid until revoked.".into(),
                benefits: vec!["Clients do not need a refresh flow.".into()],
                costs: vec!["Revocation remains the only normal expiry.".into()],
                risks: vec!["Stolen active tokens remain useful longer.".into()],
                consequences: vec!["Existing client behavior remains unchanged.".into()],
                reversibility: "A later policy change requires client updates.".into(),
            },
        ],
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["This affects every signed-in client.".into()],
        reversibility: "The choice can be revisited after client support ships.".into(),
        defer_consequence: "The security contract remains unresolved.".into(),
        evidence: vec!["api/src/tokens.rs documents persistent tokens.".into()],
        adr_assessment: Some(AdrAssessment {
            create: true,
            title: "Expire API tokens".into(),
            rationale: "Token lifetime is a durable security contract.".into(),
            revisit_when: vec!["The threat model changes.".into()],
        }),
    }
}

fn sample_items() -> Vec<OpenItem> {
    let mut v = vec![
        OpenItem::new(
            "CLR-001".into(),
            Priority::Blocking,
            ItemKind::Ambiguity,
            "Security".into(),
            Some("InfoSec".into()),
            "Should API tokens expire automatically?\nOr live until revoked?".into(),
            "Token creation defined, lifetime is not.".into(),
        ),
        OpenItem::new(
            "CLR-002".into(),
            Priority::Normal,
            ItemKind::Question,
            "Product".into(),
            None,
            "What happens when an export is interrupted?".into(),
            "Unspecified in the conversation.".into(),
        ),
    ];
    v[1].blocked_by = vec!["CLR-001".into()];
    v[0].authority = Authority::Review;
    v[0].feature_id = Some("CHG-001".into());
    v[0].feature_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    v[0].recommendation =
        "Use expiring tokens; a reviewer can approve this provisional direction.".into();
    v[0].evidence = "api/src/tokens.rs documents the current persistent token behavior.".into();
    v[0].decision_brief = Some(decision_brief("CLR-001"));
    sort_queue(&mut v);
    v
}

#[test]
fn serialize_parse_round_trip_preserves_everything() {
    for _ in 0..2 {
        let items = sample_items();
        let md = serialize(&items);
        let back = parse(&md).expect("parse own output");
        assert_eq!(back, items);
    }
}

#[test]
fn legacy_items_without_packet_identity_load_for_migration() {
    let current = serialize(&sample_items());
    let legacy = current
        .lines()
        .filter(|line| {
            !line.starts_with("**UID:**")
                && !line.starts_with("**Feature UID:**")
                && !line.starts_with("**Decision Brief:**")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let parsed = parse(&legacy).unwrap();
    assert!(parsed.iter().all(|item| item.uid.is_none()));
    assert!(parsed.iter().all(|item| item.feature_uid.is_none()));
}

#[test]
fn empty_queue_marker_survives() {
    let md = serialize(&[]);
    assert!(parse(&md).unwrap().is_empty());
    let with = serialize(&sample_items());
    assert!(!with.contains("_No open items"));
}

#[test]
fn rejects_duplicate_ids_and_missing_question() {
    let bad = "## CLR-001\n\n**Priority:** High\n**Type:** Question\n**Category:** QA\n**Assigned To:** Taylor\n\n### Question\nA?\n\n### Reason\nr\n\n## CLR-001\n\n**Priority:** High\n**Type:** Question\n**Category:** QA\n\n### Question\nB?\n\n### Reason\nr\n";
    assert!(parse(bad).is_err());
    let missing = "## CLR-007\n\n**Priority:** High\n**Type:** Question\n**Category:** QA\n\n### Reason\nforgot question\n";
    assert!(parse(missing).is_err());
}

#[test]
fn field_lookalike_inside_question_does_not_break_parse() {
    let md = "## CLR-009\n\n**Priority:** High\n**Type:** Question\n**Category:** QA\n**Assigned To:** (unassigned)\n\n### Question\n**Note:** bold prose here must stay in the question.\nSecond line.\n\n### Reason\nr\n";
    let v = parse(md).unwrap();
    assert!(v[0].question.starts_with("**Note:**"));
    assert!(v[0].question.contains("Second line."));
}

#[test]
fn unassigned_placeholder_parses_to_none() {
    let md = "## CLR-003\n\n**Priority:** High\n**Type:** Assumption\n**Category:** Ops\n**Assigned To:** (unassigned)\n\n### Question\nAssumed? \n\n### Reason\nWhy\n";
    let v = parse(md).unwrap();
    assert_eq!(v[0].assigned_to, None);
    assert_eq!(v[0].reason, "Why");
}
