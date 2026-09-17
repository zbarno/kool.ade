//! Direct board decisions use the same envelope validator and artifact
//! transaction as conversation turns. The board never stores parallel truth.
use crate::{
    core::{apply, gitops, state::PlannerState, validation},
    domain::Authority,
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
    anyhow::ensure!(
        !item.recommendation.trim().is_empty(),
        "Review item has no recommendation"
    );
    let feature_id = item
        .feature_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Review item has no related feature"))?;
    let path = crate::artifacts::product_docs::document_path(
        &state.repo_root,
        &format!("feature:{feature_id}"),
    )?;
    let feature = std::fs::read_to_string(path)?;
    let revised = insert_decision(&feature, id, &item.recommendation, &item.evidence)?;
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
        interview: None,
        task_stories: None,
        task_outline: None,
    };
    let normalized = validation::validate(&envelope, state, &state.effective_user())
        .map_err(|problems| anyhow::anyhow!(problems.join("; ")))?;
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
    use crate::domain::{ItemKind, OpenItem, Priority};

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
        std::fs::create_dir_all(repo.join("planning/features/CHG-001-saved-searches")).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.test"]);
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(repo.join("planning/specification.md"), &legacy).unwrap();
        crate::artifacts::product_docs::migrate(&repo, &legacy).unwrap();
        let feature = "# CHG-001: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo saved searches.\n\n## Desired Behavior\n\nSearches can be saved.\n\n## Scope\n\nSearch UI.\n\n## Affected Product Areas\n\n`product:05-functional-requirements`\n\n## Requirements\n\nSave search.\n\n## Decisions and Assumptions\n\nAwaiting review.\n\n## Acceptance Criteria\n\nSaved search reopens.\n";
        let feature_path = repo.join("planning/features/CHG-001-saved-searches/specification.md");
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
        item.recommendation = "Keep the last ten searches until the user deletes them.".into();
        item.evidence = "src/search.rs currently stores only the active query.".into();
        std::fs::write(
            repo.join("planning/open-items.md"),
            crate::artifacts::items_io::serialize(&[item]),
        )
        .unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "seed"]);
        let before =
            std::fs::read(repo.join("planning/product/05-functional-requirements.md")).unwrap();
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
        assert_eq!(
            std::fs::read(repo.join("planning/product/05-functional-requirements.md")).unwrap(),
            before
        );
        assert!(
            crate::artifacts::items_io::parse(
                &std::fs::read_to_string(repo.join("planning/open-items.md")).unwrap()
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
        let _ = std::fs::remove_dir_all(repo);
    }
}
