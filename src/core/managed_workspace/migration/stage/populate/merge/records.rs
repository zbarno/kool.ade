use std::collections::BTreeMap;

use serde::Serialize;

use super::super::sources::SourceData;
use super::record_conflict;
use crate::core::managed_workspace::migration::inventory::BranchConflict;
use crate::{core::planning_work::Work, domain::OpenItem};

pub(super) fn merge_work(sources: &[SourceData], conflicts: &mut Vec<BranchConflict>) -> Vec<Work> {
    let work = merge_records(
        sources,
        |source| {
            source
                .work
                .iter()
                .map(|record| (record.uid.clone(), record.clone()))
                .collect()
        },
        "state/work",
        "work-record-divergence",
        conflicts,
    );
    detect_work_key_conflicts(sources, conflicts);
    work
}

fn merge_records<T, K, F>(
    sources: &[SourceData],
    get_records: F,
    path: &str,
    kind: &str,
    conflicts: &mut Vec<BranchConflict>,
) -> Vec<T>
where
    T: Clone + PartialEq + Serialize,
    K: Ord + Clone + ToString,
    F: Fn(&SourceData) -> BTreeMap<K, T>,
{
    let mut entries = BTreeMap::<K, Vec<(T, &SourceData)>>::new();
    let mut order = Vec::new();
    for source in sources {
        for (key, value) in get_records(source) {
            if !entries.contains_key(&key) {
                order.push(key.clone());
            }
            entries.entry(key).or_default().push((value, source));
        }
    }
    let mut merged = Vec::with_capacity(entries.len());
    for key in order {
        let versions = &entries[&key];
        let first = &versions[0].0;
        if versions.iter().any(|(value, _)| value != first) {
            conflicts.push(record_conflict(
                &format!("{path}/{}.json", key.to_string()),
                key.to_string(),
                kind,
                versions
                    .iter()
                    .map(|(value, source)| (*source, value))
                    .collect(),
            ));
        }
        merged.push(first.clone());
    }
    merged
}

pub(super) fn merge_items(
    sources: &[SourceData],
    conflicts: &mut Vec<BranchConflict>,
) -> (Vec<OpenItem>, Vec<OpenItem>) {
    fn collect(
        sources: &[SourceData],
        resolved: bool,
        conflicts: &mut Vec<BranchConflict>,
    ) -> Vec<OpenItem> {
        let mut entries = BTreeMap::<String, Vec<(OpenItem, &SourceData)>>::new();
        let mut order = Vec::new();
        for source in sources {
            let records = if resolved {
                &source.resolved_items
            } else {
                &source.open_items
            };
            for item in records {
                let key = item
                    .uid
                    .clone()
                    .unwrap_or_else(|| format!("legacy:{}", item.id));
                if !entries.contains_key(&key) {
                    order.push(key.clone());
                }
                entries.entry(key).or_default().push((item.clone(), source));
            }
        }
        let mut merged = Vec::with_capacity(entries.len());
        for key in order {
            let versions = &entries[&key];
            let first = &versions[0].0;
            if versions.iter().any(|(value, _)| value != first) {
                conflicts.push(record_conflict(
                    &format!("planning/items/{key}.md"),
                    key.clone(),
                    if resolved {
                        "resolved-item-divergence"
                    } else {
                        "open-item-divergence"
                    },
                    versions
                        .iter()
                        .map(|(value, source)| (*source, value))
                        .collect(),
                ));
            }
            merged.push(first.clone());
        }
        merged
    }

    let open = collect(sources, false, conflicts);
    let resolved = collect(sources, true, conflicts);
    detect_item_state_conflicts(sources, conflicts);
    detect_item_display_id_conflicts(sources, conflicts);
    (open, resolved)
}

fn detect_item_state_conflicts(sources: &[SourceData], conflicts: &mut Vec<BranchConflict>) {
    let mut entries = BTreeMap::<String, Vec<(&SourceData, &OpenItem, &'static str)>>::new();
    for source in sources {
        for item in &source.open_items {
            let key = item
                .uid
                .clone()
                .unwrap_or_else(|| format!("legacy:{}", item.id));
            entries.entry(key).or_default().push((source, item, "open"));
        }
        for item in &source.resolved_items {
            let key = item
                .uid
                .clone()
                .unwrap_or_else(|| format!("legacy:{}", item.id));
            entries
                .entry(key)
                .or_default()
                .push((source, item, "resolved"));
        }
    }

    for (uid, versions) in entries {
        if !versions.iter().any(|(_, _, status)| *status == "open")
            || !versions.iter().any(|(_, _, status)| *status == "resolved")
        {
            continue;
        }
        let path = format!("planning/items/{uid}.md");
        let sources = versions
            .into_iter()
            .map(|(source, item, status)| {
                let artifact = item_source_artifact(source, item, status)
                    .expect("loaded item must have an inventoried source artifact");
                crate::core::managed_workspace::migration::inventory::BranchArtifact {
                    repository_id: source.checkout.repository_id.clone(),
                    source_ref: source.checkout.git_ref.clone(),
                    source_commit: source.checkout.commit.clone(),
                    source_path: artifact.mapping.source_path.clone(),
                    sha256: artifact.sha256.clone(),
                    disposition: format!("record-source-{status}"),
                    record_ids: vec![uid.clone()],
                    target_path: Some(path.clone()),
                }
            })
            .collect();
        conflicts.push(BranchConflict {
            artifact_path: path,
            record_id: Some(uid),
            kind: "item-state-divergence".into(),
            sources,
        });
    }
}

fn item_source_artifact<'a>(
    source: &'a SourceData,
    item: &OpenItem,
    status: &str,
) -> Option<&'a crate::core::managed_workspace::migration::inventory::Artifact> {
    let record_path = item
        .uid
        .as_deref()
        .map(|uid| format!("state/items/{uid}.json"));
    source
        .artifacts
        .iter()
        .find(|artifact| record_path.as_deref() == Some(artifact.relative.as_str()))
        .or_else(|| {
            let path = if status == "open" {
                "planning/open-items.md"
            } else {
                "planning/resolved-items.json"
            };
            source
                .artifacts
                .iter()
                .find(|artifact| artifact.relative == path)
        })
}

fn detect_work_key_conflicts(sources: &[SourceData], conflicts: &mut Vec<BranchConflict>) {
    let mut entries = BTreeMap::<String, Vec<(&SourceData, &Work)>>::new();
    for source in sources {
        for work in &source.work {
            entries
                .entry(work.key.clone())
                .or_default()
                .push((source, work));
        }
    }
    for (key, versions) in entries {
        let first_uid = &versions[0].1.uid;
        if versions.iter().any(|(_, work)| &work.uid != first_uid) {
            conflicts.push(record_conflict(
                &format!("state/work/{first_uid}.json"),
                key,
                "duplicate-work-key",
                versions,
            ));
        }
    }
}

fn detect_item_display_id_conflicts(sources: &[SourceData], conflicts: &mut Vec<BranchConflict>) {
    let mut entries = BTreeMap::<String, Vec<(&SourceData, &OpenItem)>>::new();
    for source in sources {
        for item in source.open_items.iter().chain(&source.resolved_items) {
            entries
                .entry(item.id.clone())
                .or_default()
                .push((source, item));
        }
    }
    for (id, versions) in entries {
        if versions
            .iter()
            .any(|(_, item)| item.uid != versions[0].1.uid)
        {
            let uid = versions[0].1.uid.as_deref().unwrap_or(id.as_str());
            conflicts.push(record_conflict(
                &format!("planning/items/{uid}.md"),
                id,
                "duplicate-item-display-id",
                versions,
            ));
        }
    }
}
