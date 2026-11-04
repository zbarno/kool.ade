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
    std::fs::write(repo.join("README.md"), "Synthetic source snapshot\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "Synthetic source baseline"]);
    crate::core::state::test_support::seed_published_product_document(&repo).unwrap();
    let project_id = crate::core::state::test_support::project_id_for_code_root(&repo);
    let local_root = repo.with_file_name(format!(
        "{}-local",
        repo.file_name().unwrap().to_string_lossy()
    ));
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project_id,
        &local_root,
        &repo,
    );
    let feature = crate::test_support::current_change_fixture(
        "# CHG-001: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo saved searches.\n\n## Desired Behavior\n\nSearches can be saved.\n\n## Scope\n\nSearch UI.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSave search.\n\n## Decisions and Assumptions\n\nAwaiting review.\n\n## Acceptance Criteria\n\nSaved search reopens.\n",
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let feature_path = repo
        .join("docs/koolade/features")
        .join(format!("{}.md", identity.uid));
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, &feature).unwrap();
    std::fs::write(
        feature_path.parent().unwrap().join("AGENTS.md"),
        format!(
            "# Navigation\n\n- [CHG-001: Saved searches](./{}.md) - Synthetic published feature.\n",
            identity.uid
        ),
    )
    .unwrap();
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Assumption,
        "Product".into(),
        "Which retention period?".into(),
        "Needs a provisional product decision.".into(),
    );
    item.uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
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
    let item_state = PlannerState::load_with_store(&repo, &store).unwrap();
    let (changes, checks) =
        crate::artifacts::items_io::record_changes(&item_state.planning_store, &[item], &[])
            .unwrap();
    item_state
        .planning_store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap();
    git(&repo, &["add", "docs/koolade/features"]);
    git(&repo, &["commit", "-qm", "Seed published feature"]);
    let head_before = git(&repo, &["rev-parse", "HEAD"]);
    let mut state = PlannerState::load_with_store(&repo, &store).unwrap();
    let _revision = approve_review(&mut state, "CLR-001").unwrap();
    assert!(state.items.is_empty());
    let reopened = PlannerState::load_with_store(&repo, &store).unwrap();
    assert_eq!(reopened.resolved_items[0].conversation_key(), "CLR-001");
    assert!(
        reopened.resolved_items[0]
            .evidence
            .contains("Approved provisional decision CLR-001")
    );
    assert!(
        crate::artifacts::product_docs::current_document(&state.planning_store, "feature:CHG-001")
            .unwrap()
            .unwrap()
            .contains("### CLR-001 — Approved provisional decision")
    );
    let approved_feature =
        crate::artifacts::product_docs::current_document(&state.planning_store, "feature:CHG-001")
            .unwrap()
            .unwrap();
    assert!(approved_feature.contains("Keep the last ten searches"));
    assert!(approved_feature.contains("src/search.rs currently stores only the active query."));
    let decision_dir = local_root.join("planning/decisions");
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
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head_before);
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
    let draft = crate::core::apply::load_current(&state.planning_store)
        .unwrap()
        .expect("board approval writes a private specification proposal");
    assert!(draft.changes.iter().any(|change| {
        change.document_id == "feature:CHG-001"
            && change
                .content
                .contains("### CLR-001 — Approved provisional decision")
    }));
    let published = state.planning_store.published_spec_pin.as_ref().unwrap();
    let published_feature = crate::core::published_specs::PublishedSpecReader::new(
        &repo,
        published.repository_id.clone(),
        published.reference.clone(),
    )
    .unwrap()
    .read(
        published,
        &format!("docs/koolade/features/{}.md", identity.uid),
    )
    .unwrap();
    assert_eq!(published_feature.content, feature);
    let (open_items, resolved_items, _) = crate::artifacts::items_io::load_store(&store).unwrap();
    assert!(open_items.is_empty());
    assert_eq!(resolved_items[0].id, "CLR-001");
    let _ = std::fs::remove_dir_all(repo);
    let _ = std::fs::remove_dir_all(local_root);
}
