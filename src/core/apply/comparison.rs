use super::ApplyReceipt;
use crate::core::state::PlannerState;

pub(super) fn apply(
    state: &mut PlannerState,
    comparison: &crate::domain::PlanComparison,
    transcript: &str,
) -> anyhow::Result<ApplyReceipt> {
    let (id, body) = state
        .active_feature
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Plan comparison has no feature target"))?;
    let path =
        crate::artifacts::product_docs::document_path(&state.repo_root, &format!("feature:{id}"))?;
    anyhow::ensure!(
        crate::artifacts::read_utf8_lossy(&path)? == body,
        "Feature changed before the plan comparison was saved"
    );
    let mut workflow = crate::artifacts::task_docs::load_workflow(&state.repo_root)?;
    anyhow::ensure!(
        workflow == state.workflow,
        "Workflow changed before the plan comparison was saved"
    );
    let mut history = workflow
        .plan_comparisons
        .remove(&id)
        .map(|previous| {
            let mut history = previous.history;
            history.push(previous.alternatives);
            history
        })
        .unwrap_or_default();
    history.dedup_by(|previous, next| previous == next);
    let record = crate::core::workflow::PlanComparisonRecord {
        schema_version: 1,
        feature_id: id.clone(),
        alternatives: comparison.clone(),
        history,
        transcript: transcript.to_owned(),
        status: crate::core::workflow::PlanComparisonStatus::Proposed,
        selected_plan: None,
        updated_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    };
    record.validate()?;
    workflow.plan_comparisons.insert(id.clone(), record);
    let layout = crate::artifacts::layout::ArtifactLayout::new(&state.repo_root);
    let path = layout.workflow_state();
    let relative = path
        .strip_prefix(&state.repo_root)?
        .to_string_lossy()
        .replace('\\', "/");
    let json = serde_json::to_string_pretty(&workflow)?;
    crate::artifacts::atomic_write(&path, &json)?;
    state.workflow = workflow;
    Ok(ApplyReceipt {
        spec_written: false,
        items_written: false,
        commit_message: format!("planner: compare implementation plans for {id}"),
        repo_relative_paths: vec![relative],
        synthesized_open_items: Vec::new(),
    })
}
