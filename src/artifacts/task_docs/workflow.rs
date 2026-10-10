use crate::artifacts::planning_store::{PlanningRoot, StoreError, paths};
use crate::core::workflow::Workflow;
use std::path::Path;

/// Refuse links in app-owned paths before creating directories or files.
pub fn safe_directory<R: PlanningRoot + ?Sized>(repo: &R, relative: &str) -> anyhow::Result<()> {
    let layout = repo.planning_layout();
    let relative = relative
        .strip_prefix(".koolade-packet/")
        .unwrap_or(relative);
    let root = layout.root();
    if !root.exists() {
        std::fs::create_dir_all(root)?;
    }
    let root_metadata = std::fs::symlink_metadata(root)?;
    anyhow::ensure!(
        root_metadata.is_dir() && !root_metadata.file_type().is_symlink(),
        "{} must be a real directory",
        root.display()
    );
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Invalid planning directory"
        );
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "{} must be a real directory",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn save_workflow<R: PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &Workflow,
) -> anyhow::Result<()> {
    safe_directory(repo, paths::STATE)?;
    repo.write_planning(paths::WORKFLOW, &serde_json::to_vec_pretty(workflow)?)?;
    Ok(())
}

pub fn load_workflow<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Workflow> {
    match repo.read_planning(paths::WORKFLOW) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)?;
            let workflow: Workflow = serde_json::from_str(&text)?;
            for (feature_id, record) in &workflow.plan_comparisons {
                anyhow::ensure!(
                    feature_id == &record.feature_id,
                    "Comparison workflow key does not match its feature ID"
                );
                record.validate()?;
            }
            promote_legacy_comparisons(repo, workflow)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            promote_legacy_comparisons(repo, Workflow::default())
        }
        Err(e) => Err(e.into()),
    }
}

/// Reconstruct old feature-metadata comparisons into the authoritative
/// workflow index. The next workflow write persists this migration; repeated
/// loads remain deterministic until then.
fn promote_legacy_comparisons<R: PlanningRoot + ?Sized>(
    repo: &R,
    mut workflow: Workflow,
) -> anyhow::Result<Workflow> {
    for (feature_id, markdown) in crate::artifacts::product_docs::active_features(repo) {
        if workflow.plan_comparisons.contains_key(&feature_id) {
            continue;
        }
        let metadata = crate::domain::ChangeMetadata::require_markdown(&markdown)?;
        if metadata.schema_version != 2 {
            continue;
        }
        let mut history = metadata.comparison_history;
        let (mut alternatives, status, selected_plan) =
            if let Some(mut comparison) = metadata.plan_comparison {
                let selected = comparison
                    .selected_plan
                    .clone()
                    .or(metadata.selected_alt.clone());
                comparison.selected_plan = selected.clone();
                let status = if selected.is_some() {
                    crate::core::workflow::PlanComparisonStatus::Adopted
                } else {
                    crate::core::workflow::PlanComparisonStatus::Proposed
                };
                (comparison, status, selected)
            } else if let Some(mut last) = history.pop() {
                last.selected_plan = None;
                (
                    last,
                    crate::core::workflow::PlanComparisonStatus::Discarded,
                    None,
                )
            } else {
                continue;
            };
        alternatives.selected_plan = selected_plan.clone();
        let all_evidence = history
            .iter()
            .cloned()
            .chain(std::iter::once(alternatives.clone()))
            .collect::<Vec<_>>();
        let mut retained_history = Vec::new();
        let mut legacy_history = Vec::new();
        for comparison in history.drain(..) {
            if crate::core::validation::validate_persisted(&comparison).is_ok() {
                retained_history.push(comparison);
            } else {
                legacy_history.push(comparison);
            }
        }
        let record = crate::core::workflow::PlanComparisonRecord {
            schema_version: 1,
            feature_id: feature_id.clone(),
            alternatives,
            history: retained_history,
            transcript: String::new(),
            status,
            selected_plan,
            updated_at_ms: 0,
        };
        if record.validate().is_ok() {
            workflow.plan_comparisons.insert(feature_id.clone(), record);
        } else {
            legacy_history = all_evidence;
        }
        if !legacy_history.is_empty() {
            workflow
                .legacy_plan_comparison_evidence
                .insert(feature_id, legacy_history);
        }
    }
    Ok(workflow)
}
