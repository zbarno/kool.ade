use crate::artifacts::planning_store::{PlanningRoot, RecordRevisionCheck, StoreError, paths};
use crate::core::workflow::Workflow;
use serde::{Deserialize, Serialize};

mod scope;
pub(super) use scope::next_batch_timestamp;
use scope::{
    batch_feature_id, batch_matches_feature, existing_times, feature_ids, feature_uid, is_empty,
    legacy_exists, matching_feature_id, merge, normalize_legacy_batch_order, now_ms,
    validate_scope, with_creation_order,
};

const RECORD_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
#[path = "record_files/tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowRecord {
    schema_version: u32,
    uid: String,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    feature_id: Option<String>,
    data: Workflow,
}

pub fn load<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Workflow> {
    let store = repo.planning_store();
    store.with_consistent_read(|| load_unlocked(repo))
}

pub(super) fn load_unlocked<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Workflow> {
    let store = repo.planning_store();
    let files = store.list_files(paths::WORKFLOW_RECORDS)?;
    if files.is_empty() {
        return load_legacy(repo);
    }
    let mut workflow = Workflow::default();
    for file in files {
        let uid = file.name.strip_suffix(".json").ok_or_else(|| {
            anyhow::anyhow!(
                "Unexpected file in {}: {}",
                paths::WORKFLOW_RECORDS,
                file.name
            )
        })?;
        let uid = uuid::Uuid::parse_str(uid)
            .map_err(|_| anyhow::anyhow!("Invalid workflow record filename {}", file.name))?
            .hyphenated()
            .to_string();
        let path = format!("{}/{uid}.json", paths::WORKFLOW_RECORDS);
        let record: WorkflowRecord = serde_json::from_slice(&store.read(&path)?)?;
        anyhow::ensure!(
            record.schema_version == RECORD_SCHEMA_VERSION,
            "Unsupported workflow record schema version {} in {path}",
            record.schema_version
        );
        anyhow::ensure!(record.uid == uid, "Workflow UID mismatch in {path}");
        anyhow::ensure!(
            record.revision > 0,
            "Workflow record {path} has revision zero"
        );
        anyhow::ensure!(
            record.created_at_ms <= record.updated_at_ms,
            "Workflow record {path} has invalid timestamps"
        );
        validate_scope(&record, &path)?;
        for (feature_id, comparison) in &record.data.plan_comparisons {
            anyhow::ensure!(
                feature_id == &comparison.feature_id,
                "Comparison workflow key does not match its feature ID"
            );
            comparison.validate()?;
        }
        merge(&mut workflow, &record.data, &path)?;
        workflow
            .record_revisions
            .insert(uid.clone(), record.revision);
        workflow
            .record_baselines
            .insert(uid.clone(), serde_json::to_string(&record.data)?);
        if let Some(feature_id) = record.feature_id {
            workflow.feature_record_ids.insert(feature_id, uid);
        }
    }
    Ok(workflow)
}

fn load_legacy<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Workflow> {
    match repo.read_planning(paths::WORKFLOW) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)?;
            let mut workflow: Workflow = serde_json::from_str(&text)?;
            normalize_legacy_batch_order(&mut workflow);
            for (feature_id, record) in &workflow.plan_comparisons {
                anyhow::ensure!(
                    feature_id == &record.feature_id,
                    "Comparison workflow key does not match its feature ID"
                );
                record.validate()?;
            }
            super::workflow::promote_legacy_comparisons(repo, workflow)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            super::workflow::promote_legacy_comparisons(repo, Workflow::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn changes<R: PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &Workflow,
) -> anyhow::Result<crate::artifacts::planning_store::RecordChangePlan> {
    let store = repo.planning_store();
    let migrating = workflow.record_revisions.is_empty() && legacy_exists(&store)?;
    let mut by_uid = std::collections::BTreeMap::<String, (Option<String>, Workflow)>::new();

    for (id, uid) in &workflow.feature_record_ids {
        by_uid.insert(uid.clone(), (Some(id.clone()), Workflow::default()));
    }
    let mut ids = feature_ids(workflow);
    let brief_feature_id = workflow.brief.as_ref().and_then(|brief| {
        matching_feature_id(repo, &brief.feature_name)
            .ok()
            .flatten()
    });
    let batch_order_base = now_ms();
    if let Some(id) = &brief_feature_id {
        ids.insert(id.clone());
    }
    for batch in &workflow.task_batches {
        if let Some(id) = batch_feature_id(repo, batch)? {
            ids.insert(id);
        }
    }
    for id in ids {
        let uid = workflow
            .feature_record_ids
            .get(&id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| feature_uid(repo, &id))?;
        by_uid
            .entry(uid)
            .or_insert_with(|| (Some(id.clone()), Workflow::default()));
    }

    let feature_uids = by_uid
        .iter()
        .filter_map(|(uid, (id, _))| id.as_ref().map(|id| (id.clone(), uid.clone())))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (uid, (feature_id, data)) in &mut by_uid {
        let Some(feature_id) = feature_id else {
            continue;
        };
        if brief_feature_id.as_deref() == Some(feature_id) {
            data.brief = workflow.brief.clone();
            data.reviewed_specification = workflow.reviewed_specification.clone();
        }
        if let Some(value) = workflow.approved_features.get(feature_id) {
            data.approved_features
                .insert(feature_id.clone(), value.clone());
        }
        if let Some(value) = workflow.plan_comparisons.get(feature_id) {
            data.plan_comparisons
                .insert(feature_id.clone(), value.clone());
        }
        if let Some(value) = workflow.feature_branch_targets.get(feature_id) {
            data.feature_branch_targets
                .insert(feature_id.clone(), value.clone());
        }
        if let Some(value) = workflow.legacy_plan_comparison_evidence.get(feature_id) {
            data.legacy_plan_comparison_evidence
                .insert(feature_id.clone(), value.clone());
        }
        data.task_batches.extend(
            workflow
                .task_batches
                .iter()
                .enumerate()
                .filter(|(_, batch)| batch_matches_feature(repo, batch, feature_id, uid))
                .map(|(index, batch)| with_creation_order(batch, batch_order_base, index)),
        );
    }

    let mut workspace = Workflow::default();
    if brief_feature_id.is_none() {
        workspace.brief = workflow.brief.clone();
        workspace.reviewed_specification = workflow.reviewed_specification.clone();
    }
    workspace.task_batches.extend(
        workflow
            .task_batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| {
                !feature_uids
                    .iter()
                    .any(|(id, uid)| batch_matches_feature(repo, batch, id, uid))
            })
            .map(|(index, batch)| with_creation_order(batch, batch_order_base, index)),
    );
    let workspace_uid = store.project_id.hyphenated().to_string();
    if !is_empty(&workspace) || workflow.record_revisions.contains_key(&workspace_uid) {
        by_uid.entry(workspace_uid).or_insert((None, workspace));
    }

    // Existing records remain represented, even if their last field was cleared.
    for (uid, revision) in &workflow.record_revisions {
        if *revision > 0 {
            by_uid
                .entry(uid.clone())
                .or_insert_with(|| (None, Workflow::default()));
        }
    }

    let mut writes = Vec::new();
    let mut checks = Vec::new();
    let now = now_ms();
    for (uid, (feature_id, data)) in by_uid {
        let serialized = serde_json::to_string(&data)?;
        if !migrating && workflow.record_baselines.get(&uid) == Some(&serialized) {
            continue;
        }
        let expected_revision = workflow.record_revisions.get(&uid).copied().unwrap_or(0);
        let path = format!("{}/{uid}.json", paths::WORKFLOW_RECORDS);
        let (created_at_ms, updated_at_ms) = existing_times(&store, &path)?
            .filter(|(_, _, revision)| *revision == expected_revision)
            .map(|(created, updated, _)| (created, now.max(updated).max(created)))
            .unwrap_or((now, now));
        let record = WorkflowRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            uid: uid.clone(),
            revision: expected_revision.saturating_add(1),
            created_at_ms,
            updated_at_ms,
            feature_id,
            data,
        };
        writes.push((path.clone(), serde_json::to_vec_pretty(&record)?));
        checks.push(RecordRevisionCheck {
            path,
            expected_revision,
        });
    }
    Ok((writes, checks, migrating))
}
