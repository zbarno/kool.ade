use super::*;
mod confirmation;

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
    approve_feature_if_current_with_snapshot_with_revision(
        repo,
        workflow,
        id,
        expected_contract,
        None,
        expected_revision,
    )
}

pub fn approve_feature_if_current_with_snapshot_with_revision<
    R: crate::artifacts::planning_store::PlanningRoot + ?Sized,
>(
    repo: &R,
    workflow: &mut Workflow,
    id: &str,
    expected_contract: Option<&str>,
    expected_snapshot_hash: Option<&str>,
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
    let identity = crate::domain::ArtifactIdentity::from_markdown(&text)?
        .ok_or_else(|| anyhow::anyhow!("Feature has no stable artifact identity"))?;
    let plan_snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &store, id, &identity, &text, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        expected_snapshot_hash.is_none_or(|expected| expected == plan_snapshot.content_hash),
        "The feature changed since its plan snapshot was displayed. Refresh and review the current plan."
    );
    let snapshot_change =
        crate::core::lifecycle::plan_snapshot::record_change(&store, &plan_snapshot)?;
    let confirmation_change = confirmation::record_user_approval(&store, &plan_snapshot)?;
    // Another conversation may have saved a brief or another approval since display.
    *workflow = crate::artifacts::task_docs::load_workflow(repo)?;
    workflow
        .approved_features
        .insert(id.to_string(), contract.clone());
    let (mut changes, mut checks, _) =
        crate::artifacts::task_docs::workflow_record_changes(repo, workflow)?;
    if let Some((path, bytes, check)) = snapshot_change {
        changes.push((path, bytes));
        checks.push(check);
    }
    if let Some((path, bytes, check)) = confirmation_change {
        changes.push((path, bytes));
        checks.push(check);
    }
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
    crate::persistence::feature_approvals::record(&store.git_root(), id, &contract)?;
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
    let contract = feature_contract(&text);
    if !crate::persistence::feature_approvals::contains(
        &repo.planning_store().git_root(),
        id,
        &contract,
    ) {
        return false;
    }
    let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(&text) else {
        return false;
    };
    let store = repo.planning_store();
    let Ok(snapshot) = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &store, id, &identity, &text, &contract,
    ) else {
        return false;
    };
    crate::core::lifecycle::plan_snapshot::is_persisted(&store, &snapshot).unwrap_or(false)
        && crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot)
            .unwrap_or(false)
        && crate::core::lifecycle::plan_snapshot::has_approved_review(&store, &snapshot)
            .unwrap_or(false)
}

/// Persist an independent review only when it still refers to the exact
/// currently confirmed plan snapshot. The receipt is create-only per snapshot.
pub fn record_plan_review_if_current<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
    receipt: &crate::core::lifecycle::GateReceipt,
    expected_revision: &str,
) -> anyhow::Result<String> {
    let _guard = crate::core::writer_gate::acquire();
    let store = repo.planning_store();
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
    let contract = feature_contract(&text);
    anyhow::ensure!(
        crate::persistence::feature_approvals::contains(&store.git_root(), id, &contract),
        "plan review requires current user confirmation"
    );
    let identity = crate::domain::ArtifactIdentity::from_markdown(&text)?
        .ok_or_else(|| anyhow::anyhow!("feature has no stable artifact identity"))?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &store, id, &identity, &text, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot)?,
        "plan review requires a persisted user confirmation receipt for this exact snapshot"
    );
    anyhow::ensure!(
        crate::core::lifecycle::plan_snapshot::is_persisted(&store, &snapshot)?,
        "reviewed plan snapshot is not persisted"
    );
    let Some((record_path, bytes, check)) =
        crate::core::lifecycle::plan_snapshot::review_change(&store, &snapshot, receipt)?
    else {
        return Ok(store.revision()?);
    };
    let (paths, revision) = store.transaction_with_revision_and_record_revisions(
        &[(record_path, bytes)],
        Some(expected_revision),
        &[check],
    )?;
    let result = crate::core::gitops::commit(
        &store.git_root(),
        &format!("planner: record independent plan review {id}"),
        &paths
            .iter()
            .map(|path| store.git_path(path))
            .collect::<Vec<_>>(),
    );
    result.map_err(|error| anyhow::anyhow!("review saved but checkpoint failed: {error}"))?;
    Ok(revision)
}

pub fn feature_approval_lapsed<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &Workflow,
    id: &str,
) -> bool {
    let Some(previous_contract) = workflow.approved_features.get(id) else {
        return false;
    };
    let git_root = repo.planning_store().git_root();
    if !crate::persistence::feature_approvals::contains(&git_root, id, previous_contract) {
        return false;
    }
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
    feature_contract(&text) != *previous_contract
}
