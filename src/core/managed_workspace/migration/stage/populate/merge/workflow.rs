use std::collections::BTreeMap;

use serde::Serialize;

use super::super::sources::SourceData;
use super::record_conflict;
use crate::core::managed_workspace::migration::inventory::BranchConflict;
use crate::core::workflow::Workflow;

pub(super) fn merge_workflow(
    sources: &[SourceData],
    conflicts: &mut Vec<BranchConflict>,
) -> Workflow {
    let mut workflow = Workflow::default();
    let brief_entries = sources.iter().filter_map(|source| {
        source
            .state
            .as_ref()
            .and_then(|state| state.workflow.brief.clone().map(|value| (value, source)))
    });
    workflow.brief = merge_singleton(
        brief_entries,
        "state/workflow/brief.json",
        "workflow-brief-divergence",
        conflicts,
    );
    let spec_entries = sources.iter().filter_map(|source| {
        source.state.as_ref().and_then(|state| {
            state
                .workflow
                .reviewed_specification
                .clone()
                .map(|value| (value, source))
        })
    });
    workflow.reviewed_specification = merge_singleton(
        spec_entries,
        "state/workflow/reviewed-specification.json",
        "workflow-specification-divergence",
        conflicts,
    );

    macro_rules! merge_feature_map {
        ($field:ident, $path:literal, $kind:literal) => {{
            let entries = sources
                .iter()
                .flat_map(|source| {
                    source.state.iter().flat_map(move |state| {
                        state
                            .workflow
                            .$field
                            .iter()
                            .map(move |(key, value)| (key.clone(), value.clone(), source))
                    })
                })
                .collect::<Vec<_>>();
            workflow.$field = merge_map(entries, $path, $kind, conflicts);
        }};
    }

    merge_feature_map!(approved_features, "state/workflow/", "approval-divergence");
    merge_feature_map!(
        plan_comparisons,
        "state/workflow/",
        "plan-comparison-divergence"
    );
    merge_feature_map!(
        feature_branch_targets,
        "state/workflow/",
        "branch-target-divergence"
    );
    merge_feature_map!(
        legacy_plan_comparison_evidence,
        "state/workflow/",
        "legacy-comparison-divergence"
    );
    merge_feature_map!(
        feature_record_ids,
        "state/workflow/",
        "workflow-record-identity-divergence"
    );

    let mut batches =
        BTreeMap::<String, Vec<(crate::core::workflow::TaskBatchRef, &SourceData)>>::new();
    for source in sources {
        if let Some(state) = &source.state {
            for batch in &state.workflow.task_batches {
                let key = batch
                    .identity
                    .as_ref()
                    .map(|identity| identity.uid.clone())
                    .unwrap_or_else(|| format!("legacy:{}:{}", batch.feature, batch.directory));
                batches
                    .entry(key)
                    .or_default()
                    .push((batch.clone(), source));
            }
        }
    }
    for (key, versions) in batches {
        let first = &versions[0].0;
        if versions.iter().any(|(value, _)| value != first) {
            conflicts.push(record_conflict(
                &format!("state/workflow/{key}.json"),
                key.clone(),
                "task-batch-divergence",
                versions
                    .iter()
                    .map(|(value, source)| (*source, value))
                    .collect(),
            ));
        }
        workflow.task_batches.push(first.clone());
    }
    workflow.task_batches.sort_by(|left, right| {
        left.created_at_ms.cmp(&right.created_at_ms).then_with(|| {
            left.identity
                .as_ref()
                .map(|id| &id.uid)
                .cmp(&right.identity.as_ref().map(|id| &id.uid))
        })
    });
    workflow.record_revisions.clear();
    workflow.record_baselines.clear();
    workflow
}

fn merge_singleton<'a, T, I>(
    entries: I,
    path: &str,
    kind: &str,
    conflicts: &mut Vec<BranchConflict>,
) -> Option<T>
where
    T: Clone + PartialEq + Serialize,
    I: Iterator<Item = (T, &'a SourceData)>,
{
    let values = entries.collect::<Vec<_>>();
    let first = values.first()?;
    if values.iter().any(|(value, _)| value != &first.0) {
        conflicts.push(record_conflict(
            path,
            path.to_owned(),
            kind,
            values
                .iter()
                .map(|(value, source)| (*source, value))
                .collect(),
        ));
    }
    Some(first.0.clone())
}

fn merge_map<K, V>(
    entries: Vec<(K, V, &SourceData)>,
    path: &str,
    kind: &str,
    conflicts: &mut Vec<BranchConflict>,
) -> BTreeMap<K, V>
where
    K: Ord + Clone + ToString,
    V: Clone + PartialEq + Serialize,
{
    let mut grouped = BTreeMap::<K, Vec<(V, &SourceData)>>::new();
    for (key, value, source) in entries {
        grouped.entry(key).or_default().push((value, source));
    }
    let mut merged = BTreeMap::new();
    for (key, versions) in grouped {
        let first = &versions[0].0;
        if versions.iter().any(|(value, _)| value != first) {
            let key_text = key.to_string();
            let record_id = versions
                .iter()
                .find_map(|(_, source)| stable_feature_uid(source, &key_text))
                .unwrap_or_else(|| key_text.clone());
            conflicts.push(record_conflict(
                &format!("{path}{record_id}.json"),
                record_id,
                kind,
                versions
                    .iter()
                    .map(|(value, source)| (*source, value))
                    .collect(),
            ));
        }
        merged.insert(key, first.clone());
    }
    merged
}

fn stable_feature_uid(source: &SourceData, feature_id: &str) -> Option<String> {
    if let Some(uid) = source
        .state
        .as_ref()
        .and_then(|state| state.workflow.feature_record_ids.get(feature_id))
    {
        return Some(uid.clone());
    }
    source.artifacts.iter().find_map(|artifact| {
        if !artifact.relative.starts_with("planning/changes/")
            || !artifact.relative.ends_with("metadata.json")
        {
            return None;
        }
        let metadata: serde_json::Value = serde_json::from_slice(&artifact.bytes).ok()?;
        (metadata.get("displayId")?.as_str()? == feature_id)
            .then(|| metadata.get("uid")?.as_str().map(str::to_owned))?
    })
}
