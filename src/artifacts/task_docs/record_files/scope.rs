use super::WorkflowRecord;
use crate::artifacts::planning_store::{PlanningRoot, PlanningStore, StoreError, paths};
use crate::core::workflow::{TaskBatchRef, Workflow};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn merge(target: &mut Workflow, source: &Workflow, path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        target.brief.is_none() || source.brief.is_none(),
        "More than one workflow record contains the active interview brief ({path})"
    );
    if source.brief.is_some() {
        target.brief = source.brief.clone();
        target.reviewed_specification = source.reviewed_specification.clone();
    }
    merge_map(
        "approval",
        &source.approved_features,
        &mut target.approved_features,
    )?;
    merge_map(
        "comparison",
        &source.plan_comparisons,
        &mut target.plan_comparisons,
    )?;
    merge_map(
        "branch target",
        &source.feature_branch_targets,
        &mut target.feature_branch_targets,
    )?;
    merge_map(
        "legacy comparison",
        &source.legacy_plan_comparison_evidence,
        &mut target.legacy_plan_comparison_evidence,
    )?;
    target.task_batches.extend(source.task_batches.clone());
    target.task_batches.sort_by(|left, right| {
        left.created_at_ms.cmp(&right.created_at_ms).then_with(|| {
            left.identity
                .as_ref()
                .map(|identity| identity.uid.as_str())
                .cmp(
                    &right
                        .identity
                        .as_ref()
                        .map(|identity| identity.uid.as_str()),
                )
        })
    });
    Ok(())
}

fn merge_map<T: Clone>(
    name: &str,
    from: &std::collections::BTreeMap<String, T>,
    to: &mut std::collections::BTreeMap<String, T>,
) -> anyhow::Result<()> {
    for (key, value) in from {
        anyhow::ensure!(
            !to.contains_key(key),
            "Duplicate {name} for feature {key} across workflow records"
        );
        to.insert(key.clone(), value.clone());
    }
    Ok(())
}

pub(super) fn validate_scope(record: &WorkflowRecord, path: &str) -> anyhow::Result<()> {
    for id in record
        .data
        .approved_features
        .keys()
        .chain(record.data.plan_comparisons.keys())
        .chain(record.data.feature_branch_targets.keys())
        .chain(record.data.legacy_plan_comparison_evidence.keys())
    {
        anyhow::ensure!(
            record.feature_id.as_deref() == Some(id.as_str()),
            "Workflow feature scope mismatch for {id} in {path}"
        );
    }
    if record.feature_id.is_some() {
        for batch in &record.data.task_batches {
            if let Some(parent_uid) = batch_feature_uid(batch) {
                anyhow::ensure!(
                    parent_uid == record.uid,
                    "Task batch feature UID does not match workflow record in {path}"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn feature_ids(workflow: &Workflow) -> std::collections::BTreeSet<String> {
    workflow
        .approved_features
        .keys()
        .chain(workflow.plan_comparisons.keys())
        .chain(workflow.feature_branch_targets.keys())
        .chain(workflow.legacy_plan_comparison_evidence.keys())
        .cloned()
        .collect()
}

pub(super) fn is_empty(workflow: &Workflow) -> bool {
    workflow.brief.is_none()
        && workflow.reviewed_specification.is_none()
        && workflow.task_batches.is_empty()
        && workflow.approved_features.is_empty()
        && workflow.plan_comparisons.is_empty()
        && workflow.feature_branch_targets.is_empty()
        && workflow.legacy_plan_comparison_evidence.is_empty()
}

pub(super) fn batch_feature_uid(batch: &TaskBatchRef) -> Option<String> {
    batch.identity.as_ref()?.parent_uid.clone()
}

pub(super) fn batch_feature_id<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatchRef,
) -> anyhow::Result<Option<String>> {
    if let Some(uid) = batch_feature_uid(batch)
        && let Some((id, _)) = crate::artifacts::product_docs::active_features(repo)
            .into_iter()
            .find(|(_, markdown)| {
                crate::domain::ArtifactIdentity::from_markdown(markdown)
                    .ok()
                    .flatten()
                    .is_some_and(|identity| identity.uid == uid)
            })
    {
        return Ok(Some(id));
    }
    matching_feature_id(repo, &batch.feature)
}

pub(super) fn batch_matches_feature<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatchRef,
    feature_id: &str,
    feature_uid: &str,
) -> bool {
    batch_feature_uid(batch).as_deref() == Some(feature_uid)
        || batch_feature_id(repo, batch).ok().flatten().as_deref() == Some(feature_id)
}

pub(super) fn with_creation_order(
    batch: &TaskBatchRef,
    base_ms: u64,
    index: usize,
) -> TaskBatchRef {
    let mut batch = batch.clone();
    if batch.created_at_ms == 0 {
        batch.created_at_ms = base_ms.saturating_add(index as u64);
    }
    batch
}

pub(super) fn normalize_legacy_batch_order(workflow: &mut Workflow) {
    for (index, batch) in workflow.task_batches.iter_mut().enumerate() {
        if batch.created_at_ms == 0 {
            batch.created_at_ms = (index as u64).saturating_add(1);
        }
    }
}

pub fn next_batch_timestamp(workflow: &Workflow) -> u64 {
    now_ms().max(
        workflow
            .task_batches
            .iter()
            .map(|batch| batch.created_at_ms.saturating_add(1))
            .max()
            .unwrap_or_default(),
    )
}

pub(super) fn matching_feature_id<R: PlanningRoot + ?Sized>(
    repo: &R,
    feature_name: &str,
) -> anyhow::Result<Option<String>> {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    };
    let brief = normalize(feature_name);
    for (id, markdown) in crate::artifacts::product_docs::active_features(repo) {
        let title = markdown
            .lines()
            .find_map(|line| line.strip_prefix("# "))
            .unwrap_or_default()
            .trim();
        let title = title.strip_prefix(&format!("{id}: ")).unwrap_or(title);
        let brief_without_id = feature_name.replace(&id, "");
        if normalize(title) == brief || normalize(title) == normalize(&brief_without_id) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub(super) fn feature_uid<R: PlanningRoot + ?Sized>(repo: &R, id: &str) -> anyhow::Result<String> {
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
    crate::domain::ArtifactIdentity::from_markdown(&text)?
        .map(|identity| identity.uid)
        .ok_or_else(|| anyhow::anyhow!("Feature {id} has no stable artifact identity"))
}

pub(super) fn legacy_exists(store: &PlanningStore) -> anyhow::Result<bool> {
    match store.read(paths::WORKFLOW) {
        Ok(_) => Ok(true),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn existing_times(
    store: &PlanningStore,
    path: &str,
) -> anyhow::Result<Option<(u64, u64, u64)>> {
    match store.read(path) {
        Ok(bytes) => {
            let record: WorkflowRecord = serde_json::from_slice(&bytes)?;
            Ok(Some((
                record.created_at_ms,
                record.updated_at_ms,
                record.revision,
            )))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
