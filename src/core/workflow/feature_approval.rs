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

pub fn approve_feature<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &mut Workflow,
    id: &str,
) -> anyhow::Result<String> {
    approve_feature_if_current(repo, workflow, id, None)
}

pub fn approve_feature_if_current<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &mut Workflow,
    id: &str,
    expected_contract: Option<&str>,
) -> anyhow::Result<String> {
    let store = repo.planning_store();
    let expected_revision = store.revision()?;
    let receipt = approve_feature_if_current_with_revision(
        repo,
        workflow,
        id,
        expected_contract,
        &expected_revision,
    )?;
    receipt
        .commit_result
        .map_err(|error| anyhow::anyhow!("approval saved but checkpoint failed: {error}"))
}

#[derive(Debug)]
pub struct FeatureApprovalReceipt {
    pub planning_revision: String,
    pub commit_result: Result<String, String>,
}

pub fn approve_feature_if_current_with_revision<
    R: crate::artifacts::planning_store::PlanningRoot + ?Sized,
>(
    repo: &R,
    workflow: &mut Workflow,
    id: &str,
    expected_contract: Option<&str>,
    expected_revision: &str,
) -> anyhow::Result<FeatureApprovalReceipt> {
    // Writer section: this feature-scoped record write shares the transaction
    // lock with background turns/reconciliation.
    let guard = crate::core::writer_gate::acquire();
    let store = repo.planning_store();
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
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
    let (changes, checks, _) =
        crate::artifacts::task_docs::workflow_record_changes(repo, workflow)?;
    let (paths, planning_revision) = store.transaction_with_revision_and_record_revisions(
        &changes,
        Some(expected_revision),
        &checks,
    )?;
    *workflow = crate::artifacts::task_docs::load_workflow(repo)?;
    let result = crate::core::gitops::commit(
        &store.git_root(),
        &format!("planner: approve feature {id}"),
        &paths
            .iter()
            .map(|path| store.git_path(path))
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string());
    drop(guard);
    Ok(FeatureApprovalReceipt {
        planning_revision,
        commit_result: result,
    })
}

pub fn feature_approved<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &Workflow,
    id: &str,
) -> bool {
    let Ok(path) = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))
    else {
        return false;
    };
    let Ok(bytes) = repo.read_planning_path(&path) else {
        return false;
    };
    let Ok(text) = String::from_utf8(bytes) else {
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
