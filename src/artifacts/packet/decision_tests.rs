use super::*;
use crate::domain::{DecisionBrief, DecisionOption, DecisionRecommendation, ItemKind, Priority};

fn decision(create_adr: bool) -> OpenItem {
    let mut item = OpenItem::new(
        "CLR-012".into(),
        Priority::High,
        ItemKind::Assumption,
        "Architecture".into(),
        Some("Reviewer".into()),
        "Which session retention policy should we adopt?".into(),
        "The choice determines how long user credentials remain useful.".into(),
    );
    item.authority = Authority::Review;
    item.feature_id = Some("F-012".into());
    item.decision_brief = Some(DecisionBrief {
        id: item.id.clone(),
        question: item.question.clone(),
        why_now: "The session interface depends on the selected policy.".into(),
        recommendation: Some(DecisionRecommendation {
            option_id: "expire".into(),
            rationale: "Shorter sessions reduce exposure after credential theft.".into(),
        }),
        confidence: None,
        options: vec![
            DecisionOption {
                id: "expire".into(),
                label: "Expire sessions after one hour".into(),
                summary: "Users renew a session after one hour.".into(),
                benefits: vec!["Limits the lifetime of a stolen session.".into()],
                costs: vec!["Long-running work may require another sign-in.".into()],
                risks: vec![],
                consequences: vec!["Every client must handle session expiry.".into()],
                reversibility: "The timeout can change with client support.".into(),
            },
            DecisionOption {
                id: "persistent".into(),
                label: "Keep sessions until sign-out".into(),
                summary: "Sessions remain active until users sign out.".into(),
                benefits: vec!["Users avoid unexpected repeat sign-ins.".into()],
                costs: vec!["Stolen sessions remain useful longer.".into()],
                risks: vec![],
                consequences: vec!["Clients need no expiry flow.".into()],
                reversibility: "Expiry can be added after client updates.".into(),
            },
        ],
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["All signed-in users follow this policy.".into()],
        reversibility: "The choice can be reviewed after client support ships.".into(),
        defer_consequence: "The session behavior cannot be finalized.".into(),
        evidence: vec!["src/auth/session.rs stores current session state.".into()],
        adr_assessment: Some(AdrAssessment {
            create: create_adr,
            title: if create_adr {
                "Expire sessions after one hour".into()
            } else {
                String::new()
            },
            rationale: if create_adr {
                "This establishes a lasting security and client contract.".into()
            } else {
                "This decision is local and easy to change later.".into()
            },
            revisit_when: if create_adr {
                vec!["The threat model or client refresh support changes.".into()]
            } else {
                vec![]
            },
        }),
    });
    item
}

fn repository(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("packet_decision_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    root
}

#[test]
fn ordinary_approved_choice_does_not_create_an_adr() {
    let root = repository("ordinary");
    assert!(
        prepare_decision_record(&root, &decision(false))
            .unwrap()
            .is_none()
    );
    assert!(!ArtifactLayout::new(&root).decisions_root().exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn material_approved_choice_prepares_a_compact_immutable_adr() {
    let root = repository("material");
    let item = decision(true);
    let first = prepare_decision_record(&root, &item).unwrap().unwrap();
    assert!(
        first
            .0
            .ends_with("ADR-001-expire-sessions-after-one-hour.md")
    );
    let visible = ArtifactIdentity::visible_markdown(&first.1);
    for required in [
        "Status: Accepted",
        "Related change: `F-012`",
        "## Context",
        "## Decision",
        "## Alternatives considered",
        "## Consequences",
        "## Revisit when",
        "Keep sessions until sign-out",
    ] {
        assert!(visible.contains(required), "missing {required}");
    }
    assert!(!visible.contains("## Verification"));
    assert!(!visible.contains("Acceptance evidence"));
    assert!(!visible.contains("Implementation commit"));

    crate::artifacts::transaction::apply(&root, std::slice::from_ref(&first)).unwrap();
    let retry = prepare_decision_record(&root, &item).unwrap().unwrap();
    assert_eq!(retry, first);
    assert_eq!(
        std::fs::read_to_string(root.join(&first.0)).unwrap(),
        first.1
    );
    let _ = std::fs::remove_dir_all(root);
}
