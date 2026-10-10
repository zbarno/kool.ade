use super::*;
use crate::domain::{AdrAssessment, ItemKind, OpenItem, Priority};

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn board_approval_records_feature_decision_and_resolves_item_atomically() {
    let repo = std::env::temp_dir().join(format!(
        "koolade_board_review_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    std::fs::create_dir_all(repo.join("planning")).unwrap();
    std::fs::write(repo.join("planning/specification.md"), &legacy).unwrap();
    crate::artifacts::product_docs::migrate(&repo, &legacy).unwrap();
    std::fs::create_dir_all(repo.join(".koolade-packet/planning")).unwrap();
    let feature = "# CHG-001: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo saved searches.\n\n## Desired Behavior\n\nSearches can be saved.\n\n## Scope\n\nSearch UI.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSave search.\n\n## Decisions and Assumptions\n\nAwaiting review.\n\n## Acceptance Criteria\n\nSaved search reopens.\n";
    let feature_path =
        repo.join(".koolade-packet/planning/changes/CHG-001-saved-searches/specification.md");
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, feature).unwrap();
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Assumption,
        "Product".into(),
        Some("Fixture".into()),
        "Which retention period?".into(),
        "Needs a provisional product decision.".into(),
    );
    item.authority = Authority::Review;
    item.feature_id = Some("CHG-001".into());
    item.decision_brief = Some(crate::domain::DecisionBrief {
        id: item.id.clone(),
        question: item.question.clone(),
        why_now: "The feature needs a storage policy before implementation.".into(),
        recommendation: Some(crate::domain::DecisionRecommendation {
            option_id: "keep-ten".into(),
            rationale: "This keeps useful history while bounding stored searches.".into(),
        }),
        confidence: Some(crate::domain::DecisionConfidence {
            level: crate::domain::ConfidenceLevel::Medium,
            explanation: "The code behavior is known, while user preference is not.".into(),
        }),
        options: vec![
            crate::domain::DecisionOption {
                id: "keep-ten".into(),
                label: "Keep the last ten searches".into(),
                summary: "Older searches are removed as new ones are saved.".into(),
                benefits: vec!["Keeps recent searches easy to reopen.".into()],
                costs: vec!["Older entries do not remain available.".into()],
                risks: vec![],
                consequences: vec!["The stored list never grows beyond ten.".into()],
                reversibility: "The limit can be changed later.".into(),
            },
            crate::domain::DecisionOption {
                id: "keep-all".into(),
                label: "Keep searches until deleted".into(),
                summary: "Users decide when to remove older searches.".into(),
                benefits: vec!["Search history remains available.".into()],
                costs: vec!["Stored history can keep growing.".into()],
                risks: vec![],
                consequences: vec!["Users manage storage by deleting entries.".into()],
                reversibility: "A later limit can remove older entries.".into(),
            },
        ],
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["The choice sets storage behavior for every account.".into()],
        reversibility: "The policy can be adjusted in a later release.".into(),
        defer_consequence: "The feature remains unready for implementation.".into(),
        evidence: vec!["src/search.rs currently stores only the active query.".into()],
        adr_assessment: Some(AdrAssessment {
            create: true,
            title: "Bound saved search history".into(),
            rationale: "Retention affects durable user data and future storage behavior.".into(),
            revisit_when: vec![
                "Observed search volume makes the selected retention limit unsuitable.".into(),
            ],
        }),
    });
    let decision_uid = item.uid.clone();
    std::fs::write(
        repo.join(".koolade-packet/planning/open-items.md"),
        crate::artifacts::items_io::serialize(&[item]),
    )
    .unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "seed"]);
    let before =
        std::fs::read(repo.join(".koolade-packet/planning/product/current-capabilities.md"))
            .unwrap();
    let mut state = PlannerState::load(&repo).unwrap();
    let commit = approve_review(&mut state, "CLR-001").unwrap();
    assert!(!commit.is_empty());
    assert!(state.items.is_empty());
    let reopened = PlannerState::load(&repo).unwrap();
    assert_eq!(reopened.resolved_items[0].conversation_key(), "CLR-001");
    assert!(
        reopened.resolved_items[0]
            .evidence
            .contains("Approved provisional decision CLR-001")
    );
    assert!(
        std::fs::read_to_string(&feature_path)
            .unwrap()
            .contains("### CLR-001 — Approved provisional decision")
    );
    let approved_feature = std::fs::read_to_string(&feature_path).unwrap();
    assert!(approved_feature.contains("Keep the last ten searches"));
    assert!(approved_feature.contains("src/search.rs currently stores only the active query."));
    let decision_dir = repo.join(".koolade-packet/planning/decisions");
    let decisions = std::fs::read_dir(&decision_dir)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(decisions.len(), 1);
    let adr_path = decisions[0].path();
    let adr = std::fs::read_to_string(&adr_path).unwrap();
    assert!(adr.contains("# ADR-001: Bound saved search history"));
    assert!(adr.contains("## Alternatives considered"));
    assert!(adr.contains("Keep searches until deleted"));
    assert!(adr.contains("## Revisit when"));
    assert!(!adr.contains("## Verification"));
    assert!(!adr.contains("Implementation commit"));
    let adr_identity = crate::domain::ArtifactIdentity::from_markdown(&adr)
        .unwrap()
        .unwrap();
    assert_eq!(adr_identity.parent_uid, decision_uid);
    assert!(
        git(&repo, &["show", "--pretty=format:", "--name-only", "HEAD"])
            .contains(".koolade-packet/planning/decisions/ADR-001-bound-saved-search-history.md")
    );
    assert_eq!(
        std::fs::read(repo.join(".koolade-packet/planning/product/current-capabilities.md"),)
            .unwrap(),
        before
    );
    let store =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(uuid::Uuid::nil(), &repo);
    let (open_items, resolved_items, _) = crate::artifacts::items_io::load_store(&store).unwrap();
    assert!(open_items.is_empty());
    assert_eq!(resolved_items[0].id, "CLR-001");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
    let _ = std::fs::remove_dir_all(repo);
}
