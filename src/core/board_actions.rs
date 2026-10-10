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
    let current = PlannerState::load_with_store(&state.repo_root, &state.planning_store)?;
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
        &state.planning_store,
        &format!("feature:{feature_id}"),
    )?;
    let feature = String::from_utf8(state.planning_store.read_planning_path(&path)?)?;
    let revised = insert_decision(&feature, id, &recommendation, &evidence)?;
    let envelope = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some(format!("Approved provisional decision {id}.")),
        change_summary: Some(format!("Approve review {id}")),
        document_updates: Some(vec![DocumentUpdate {
            document_id: format!("feature:{feature_id}"),
            content: revised,
            status: None,
        }]),
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: Some(vec![id.to_string()]),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        interview: None,
        task_stories: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let mut normalized = validation::validate(&envelope, state, &state.effective_user())
        .map_err(|problems| anyhow::anyhow!(problems.join("; ")))?;
    if let Some((path, content)) =
        crate::artifacts::koolade::prepare_decision_record(&state.planning_store, item)?
    {
        normalized
            .additional_planning_artifacts
            .push((path, content));
    }
    let receipt = apply::apply(state, &normalized)?;
    let result = gitops::commit(
        &state.planning_store.git_root(),
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
#[path = "board_actions/tests.rs"]
mod tests;
