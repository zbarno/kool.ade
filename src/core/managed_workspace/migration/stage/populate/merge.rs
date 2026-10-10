mod records;
mod workflow;

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::sources::SourceData;
use crate::core::managed_workspace::migration::inventory::{BranchArtifact, BranchConflict};
use crate::{core::planning_work::Work, core::workflow::Workflow, domain::OpenItem};

pub(super) struct MergedPlanning {
    pub work: Vec<Work>,
    pub workflow: Workflow,
    pub open: Vec<OpenItem>,
    pub resolved: Vec<OpenItem>,
    pub conflicts: Vec<BranchConflict>,
}

pub(super) fn merge(sources: &[SourceData]) -> MergedPlanning {
    let mut conflicts = Vec::new();
    let work = records::merge_work(sources, &mut conflicts);
    let (open, resolved) = records::merge_items(sources, &mut conflicts);
    let workflow = workflow::merge_workflow(sources, &mut conflicts);
    MergedPlanning {
        work,
        workflow,
        open,
        resolved,
        conflicts,
    }
}

pub(super) fn artifact_conflicts(sources: &[SourceData]) -> Vec<BranchConflict> {
    let mut by_target = BTreeMap::<
        String,
        Vec<(
            &SourceData,
            &crate::core::managed_workspace::migration::inventory::Artifact,
        )>,
    >::new();
    for source in sources {
        for artifact in &source.artifacts {
            if artifact.kind != crate::core::managed_workspace::migration::inventory::Kind::Shared {
                continue;
            }
            if let Some(target) = &artifact.mapping.target_path {
                by_target
                    .entry(target.clone())
                    .or_default()
                    .push((source, artifact));
            }
        }
    }
    by_target
        .into_iter()
        .filter_map(|(target, entries)| {
            let first = entries.first()?.1.sha256.as_str();
            if entries.iter().all(|(_, artifact)| artifact.sha256 == first) {
                return None;
            }
            let sources = entries
                .into_iter()
                .map(|(source, artifact)| BranchArtifact {
                    repository_id: source.checkout.repository_id.clone(),
                    source_ref: source.checkout.git_ref.clone(),
                    source_commit: source.checkout.commit.clone(),
                    source_path: artifact.mapping.source_path.clone(),
                    sha256: artifact.sha256.clone(),
                    disposition: "registered-checkout-artifact".into(),
                    record_ids: Vec::new(),
                    target_path: Some(target.clone()),
                })
                .collect();
            Some(BranchConflict {
                artifact_path: target.clone(),
                record_id: stable_record_id(&target),
                kind: "registered-checkout-artifact-divergence".into(),
                sources,
            })
        })
        .collect()
}

pub(super) fn record_conflict<T: Serialize>(
    path: &str,
    record_id: String,
    kind: &str,
    records: Vec<(&SourceData, &T)>,
) -> BranchConflict {
    BranchConflict {
        artifact_path: path.to_owned(),
        record_id: Some(record_id.clone()),
        kind: kind.to_owned(),
        sources: records
            .into_iter()
            .map(|(source, record)| BranchArtifact {
                repository_id: source.checkout.repository_id.clone(),
                source_ref: source.checkout.git_ref.clone(),
                source_commit: source.checkout.commit.clone(),
                source_path: format!(".koolade-packet/{path}"),
                sha256: digest(record),
                disposition: "record-source".into(),
                record_ids: vec![record_id.clone()],
                target_path: Some(path.to_owned()),
            })
            .collect(),
    }
}

fn stable_record_id(path: &str) -> Option<String> {
    path.rsplit_once('/')?
        .1
        .strip_suffix(".json")
        .or_else(|| path.rsplit_once('/')?.1.strip_suffix(".md"))
        .map(str::to_owned)
}

fn digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("serializing typed migration records cannot fail");
    format!("{:x}", Sha256::digest(bytes))
}
