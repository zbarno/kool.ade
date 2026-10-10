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
    let path = crate::artifacts::product_docs::document_path(
        &state.planning_store,
        &format!("feature:{id}"),
    )?;
    anyhow::ensure!(
        String::from_utf8(state.planning_store.read_planning_path(&path)?)? == body,
        "Feature changed before the plan comparison was saved"
    );
    let mut workflow = crate::artifacts::task_docs::load_workflow(&state.planning_store)?;
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
    let bytes = serde_json::to_vec_pretty(&workflow)?;
    let (paths, revision) = state.planning_store.transaction_with_revision(
        &[(
            crate::artifacts::planning_store::paths::WORKFLOW.into(),
            bytes,
        )],
        Some(&state.baseline_planning_revision),
    )?;
    let relative = paths
        .iter()
        .map(|path| state.planning_store.git_path(path))
        .collect();
    state.workflow = workflow;
    state.baseline_planning_revision = revision;
    Ok(ApplyReceipt {
        spec_written: false,
        items_written: false,
        commit_message: format!("planner: compare implementation plans for {id}"),
        repo_relative_paths: relative,
        synthesized_open_items: Vec::new(),
    })
}
