use super::*;

/// Freeze the normative feature contract, excluding mutable status and
/// repository-observation sections. A changed desired behavior needs approval again.
pub fn feature_contract(text: &str) -> String {
    let mut capture = false;
    let mut selected = String::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            capture = [
                "Intent",
                "Desired Behavior",
                "Scope",
                "Requirements",
                "Decisions and Assumptions",
                "Acceptance Criteria",
                "Selected Plan",
            ]
            .contains(&heading);
        }
        if capture {
            selected.push_str(line);
            selected.push('\n');
        }
    }
    selected
}

pub fn approve_feature(
    repo: &std::path::Path,
    workflow: &mut Workflow,
    id: &str,
) -> anyhow::Result<String> {
    approve_feature_if_current(repo, workflow, id, None)
}

pub fn approve_feature_if_current(
    repo: &std::path::Path,
    workflow: &mut Workflow,
    id: &str,
    expected_contract: Option<&str>,
) -> anyhow::Result<String> {
    // Writer section: this read-modify-commit of workflow.json shares the
    // planning index with background turns/reconciliation.
    let guard = crate::core::writer_gate::acquire();
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = std::fs::read_to_string(path)?;
    crate::core::specification::validate_feature(id, &text)?;
    let metadata = crate::domain::ChangeMetadata::require_markdown(&text)?;
    let status = metadata.status;
    anyhow::ensure!(
        status.approval_eligible(),
        "Only a ready or already implementing feature may be approved"
    );
    let current_workflow = crate::artifacts::task_docs::load_workflow(repo)?;
    let comparison_ready = current_workflow
        .plan_comparisons
        .get(id)
        .is_some_and(|record| {
            record.status == PlanComparisonStatus::Adopted && record.selected_plan.is_some()
        });
    anyhow::ensure!(
        metadata.schema_version != 2 || comparison_ready,
        "Compare and adopt a plan before approving this feature"
    );
    let contract = feature_contract(&text);
    anyhow::ensure!(!contract.trim().is_empty(), "Feature contract is empty");
    anyhow::ensure!(
        expected_contract.is_none_or(|expected| expected == contract),
        "The feature changed since it was displayed. Refresh and review its current specification."
    );
    // Another conversation may have saved a brief or another approval since display.
    *workflow = crate::artifacts::task_docs::load_workflow(repo)?;
    workflow.approved_features.insert(id.to_string(), contract);
    crate::artifacts::task_docs::save_workflow(repo, workflow)?;
    let result = crate::core::gitops::commit(
        repo,
        &format!("planner: approve feature {id}"),
        &[WORKFLOW_FILE.to_string()],
    )
    .map_err(|e| anyhow::anyhow!("approval saved but checkpoint failed: {e}"));
    drop(guard);
    result
}

pub fn feature_approved(repo: &std::path::Path, workflow: &Workflow, id: &str) -> bool {
    let Ok(path) = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))
    else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(metadata) = crate::domain::ChangeMetadata::require_markdown(&text) else {
        return false;
    };
    let comparison_ready = workflow.plan_comparisons.get(id).is_some_and(|record| {
        record.status == PlanComparisonStatus::Adopted && record.selected_plan.is_some()
    });
    if metadata.schema_version == 2 && !comparison_ready {
        return false;
    }
    workflow
        .approved_features
        .get(id)
        .is_some_and(|snapshot| snapshot == &feature_contract(&text))
}
