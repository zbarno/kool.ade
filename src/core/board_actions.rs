//! Direct board decisions use the same envelope validator and artifact
//! transaction as conversation turns. The board never stores parallel truth.
use crate::{
    core::{apply, gitops, state::PlannerState, validation},
    domain::{Authority, OpenItem},
    harness::{DocumentUpdate, TurnEnvelope},
};

fn decision_entry(id: &str, recommendation: &str, evidence: &str) -> String {
    let mut entry = format!("\n### {id} — Approved provisional decision\n\n");
    for line in recommendation.lines() {
        entry.push_str(&format!("> {}\n", line.trim()));
    }
    if !evidence.trim().is_empty() {
        entry.push_str("\nEvidence considered:\n\n");
        for line in evidence.lines() {
            entry.push_str(&format!("> {}\n", line.trim()));
        }
    }
    entry.push('\n');
    entry
}

fn insert_decision(
    feature: &str,
    id: &str,
    recommendation: &str,
    evidence: &str,
) -> anyhow::Result<String> {
    let marker = "## Acceptance Criteria";
    let position = feature
        .find(marker)
        .ok_or_else(|| anyhow::anyhow!("Feature has no acceptance section"))?;
    anyhow::ensure!(
        feature[..position].contains("## Decisions and Assumptions"),
        "Feature has no decisions section"
    );
    anyhow::ensure!(
        !feature.contains(&format!("### {id} — Approved provisional decision")),
        "Review decision already recorded"
    );
    Ok(format!(
        "{}{}{}",
        &feature[..position],
        decision_entry(id, recommendation, evidence),
        &feature[position..]
    ))
}

fn provisional_recommendation(item: &OpenItem) -> Option<String> {
    if !item.recommendation.trim().is_empty() {
        return Some(item.recommendation.clone());
    }
    let brief = item.decision_brief.as_ref()?;
    let recommendation = brief.recommendation.as_ref()?;
    let option = brief
        .options
        .iter()
        .find(|option| option.id == recommendation.option_id)?;
    Some(format!("{} — {}", option.label, recommendation.rationale))
}

fn decision_evidence(item: &OpenItem) -> String {
    if !item.evidence.trim().is_empty() {
        return item.evidence.clone();
    }
    item.decision_brief
        .as_ref()
        .map(|brief| brief.evidence.join("\n"))
        .unwrap_or_default()
}

pub fn approve_review(state: &mut PlannerState, id: &str) -> anyhow::Result<String> {
    // Writer section: board decision writes the feature spec + items and
    // checkpoints, so it joins the planning writer gate like every other
    // artifact mutator.
    let guard = crate::core::writer_gate::acquire();
    let current = PlannerState::load(&state.repo_root)?;
    let drifted = PlannerState::drift_report(state, &current);
    anyhow::ensure!(
        drifted.is_empty(),
        "Planning artifacts changed; reload the board before approving"
    );
    let item = state
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("Review item is no longer open"))?;
    anyhow::ensure!(
        item.authority == Authority::Review,
        "Only provisional review items can be approved here"
    );
    let recommendation = provisional_recommendation(item)
        .ok_or_else(|| anyhow::anyhow!("Review item has no recommendation"))?;
    let evidence = decision_evidence(item);
    let feature_id = item
        .feature_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Review item has no related feature"))?;
    let path = crate::artifacts::product_docs::document_path(
        &state.repo_root,
        &format!("feature:{feature_id}"),
    )?;
    let feature = std::fs::read_to_string(path)?;
    let revised = insert_decision(&feature, id, &recommendation, &evidence)?;
    let envelope = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some(format!("Approved provisional decision {id}.")),
        change_summary: Some(format!("Approve review {id}")),
        document_updates: Some(vec![DocumentUpdate {
            document_id: format!("feature:{feature_id}"),
            content: revised,
        }]),
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: Some(vec![id.to_string()]),
        next_question_id: None,
        requested_action: None,
        interview: None,
        task_stories: None,
        task_outline: None,
    };
    let mut normalized = validation::validate(&envelope, state, &state.effective_user())
        .map_err(|problems| anyhow::anyhow!(problems.join("; ")))?;
    if let Some((path, content)) =
        crate::artifacts::packet::prepare_decision_record(&state.repo_root, item)?
    {
        normalized
            .additional_planning_artifacts
            .push((path, content));
    }
    let receipt = apply::apply(state, &normalized)?;
    let result = gitops::commit(
        &state.repo_root,
        &receipt.commit_message,
        &receipt.repo_relative_paths,
    )
    .map_err(|error| {
        anyhow::anyhow!("Board decision was saved but git checkpoint failed: {error}")
    });
    drop(guard);
    result
}

#[cfg(test)]
mod tests {
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
            "packet_board_review_{}-{}",
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
        std::fs::create_dir_all(repo.join(".kool-ade-packet/planning")).unwrap();
        let feature = "# CHG-001: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo saved searches.\n\n## Desired Behavior\n\nSearches can be saved.\n\n## Scope\n\nSearch UI.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSave search.\n\n## Decisions and Assumptions\n\nAwaiting review.\n\n## Acceptance Criteria\n\nSaved search reopens.\n";
        let feature_path =
            repo.join(".kool-ade-packet/planning/changes/CHG-001-saved-searches/specification.md");
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
                rationale: "Retention affects durable user data and future storage behavior."
                    .into(),
                revisit_when: vec![
                    "Observed search volume makes the selected retention limit unsuitable.".into(),
                ],
            }),
        });
        let decision_uid = item.uid.clone();
        std::fs::write(
            repo.join(".kool-ade-packet/planning/open-items.md"),
            crate::artifacts::items_io::serialize(&[item]),
        )
        .unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "seed"]);
        let before =
            std::fs::read(repo.join(".kool-ade-packet/planning/product/current-capabilities.md"))
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
        let decision_dir = repo.join(".kool-ade-packet/planning/decisions");
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
            git(&repo, &["show", "--pretty=format:", "--name-only", "HEAD"]).contains(
                ".kool-ade-packet/planning/decisions/ADR-001-bound-saved-search-history.md"
            )
        );
        assert_eq!(
            std::fs::read(repo.join(".kool-ade-packet/planning/product/current-capabilities.md"),)
                .unwrap(),
            before
        );
        assert!(
            crate::artifacts::items_io::parse(
                &std::fs::read_to_string(repo.join(".kool-ade-packet/planning/open-items.md"),)
                    .unwrap()
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
        let _ = std::fs::remove_dir_all(repo);
    }
}
