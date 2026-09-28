use crate::core::workflow::Workflow;
use std::path::Path;

/// Refuse links in app-owned paths before creating directories or files.
pub fn safe_directory(repo: &Path, relative: &str) -> anyhow::Result<()> {
    let mut path = repo.to_path_buf();
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

pub fn save_workflow(repo: &Path, workflow: &Workflow) -> anyhow::Result<()> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    safe_directory(repo, crate::artifacts::layout::canonical::STATE)?;
    let target = layout.workflow_state();
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Workflow must be a regular file"
        );
    }
    crate::artifacts::atomic_write(&target, &serde_json::to_string_pretty(workflow)?)
}

pub fn load_workflow(repo: &Path) -> anyhow::Result<Workflow> {
    match std::fs::read_to_string(
        crate::artifacts::layout::ArtifactLayout::new(repo).workflow_state(),
    ) {
        Ok(text) => {
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            promote_legacy_comparisons(repo, Workflow::default())
        }
        Err(e) => Err(e.into()),
    }
}

/// Reconstruct old feature-metadata comparisons into the authoritative
/// workflow index. The next workflow write persists this migration; repeated
/// loads remain deterministic until then.
fn promote_legacy_comparisons(repo: &Path, mut workflow: Workflow) -> anyhow::Result<Workflow> {
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
