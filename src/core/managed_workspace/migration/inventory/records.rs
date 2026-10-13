use sha2::{Digest, Sha256};

use super::{Artifact, BranchArtifact};

fn stable_record_id(path: &str) -> Option<String> {
    let path = path.strip_prefix(".koolade-packet/").unwrap_or(path);
    let (collection, file) = path.rsplit_once('/')?;
    let uid = file
        .strip_suffix(".json")
        .or_else(|| file.strip_suffix(".md"))?;
    matches!(
        collection,
        "state/work"
            | "state/workflow"
            | "state/items"
            | "state/tasks"
            | "planning/items"
            | "state/questions"
            | "state/readiness"
            | "planning/questions"
    )
    .then(|| uid.to_owned())
}

pub(crate) fn stable_record_ids(path: &str, bytes: &[u8]) -> Vec<String> {
    let relative = path.strip_prefix(".koolade-packet/").unwrap_or(path);
    if let Some(uid) = stable_record_id(relative) {
        return vec![uid];
    }
    match relative {
        "state/work.json" => serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| value.get("items").cloned())
            .and_then(|items| serde_json::from_value::<Vec<serde_json::Value>>(items).ok())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("uid")?.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        "state/workflow.json" => serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .map(|workflow| {
                let mut ids = std::collections::BTreeSet::new();
                let feature_records = workflow
                    .get("featureRecordIds")
                    .and_then(|value| value.as_object());
                if let Some(records) = feature_records {
                    ids.extend(
                        records
                            .values()
                            .filter_map(|value| value.as_str().map(str::to_owned)),
                    );
                }
                let stable_feature_id = |key: &str| {
                    feature_records
                        .and_then(|records| records.get(key))
                        .and_then(|value| value.as_str())
                        .unwrap_or(key)
                        .to_owned()
                };
                for field in [
                    "approvedFeatures",
                    "featureBranchTargets",
                    "planComparisons",
                    "legacyPlanComparisonEvidence",
                ] {
                    if let Some(records) = workflow.get(field).and_then(|value| value.as_object()) {
                        ids.extend(records.keys().map(|key| stable_feature_id(key)));
                    }
                }
                if let Some(batches) = workflow
                    .get("taskBatches")
                    .and_then(|value| value.as_array())
                {
                    for identity in batches.iter().filter_map(|batch| batch.get("identity")) {
                        if let Some(uid) = identity.get("uid").and_then(|value| value.as_str()) {
                            ids.insert(uid.to_owned());
                        }
                        if let Some(uid) =
                            identity.get("parentUid").and_then(|value| value.as_str())
                        {
                            ids.insert(uid.to_owned());
                        }
                    }
                }
                ids.into_iter().collect()
            })
            .unwrap_or_default(),
        "planning/open-items.md" => std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| crate::artifacts::items_io::parse(text).ok())
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| item.uid.unwrap_or(item.id))
                    .collect()
            })
            .unwrap_or_default(),
        "planning/resolved-items.json" => {
            serde_json::from_slice::<Vec<crate::domain::OpenItem>>(bytes)
                .ok()
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| item.uid.unwrap_or(item.id))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ if relative.starts_with("planning/changes/") && relative.ends_with("metadata.json") => {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|value| value.get("uid")?.as_str().map(str::to_owned))
                .into_iter()
                .collect()
        }
        _ if relative.ends_with(".md") => std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| {
                crate::domain::ArtifactIdentity::from_markdown(text)
                    .ok()
                    .flatten()
            })
            .map(|identity| vec![identity.uid])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(crate) fn full_inventory_digest(artifacts: &[Artifact], branches: &[BranchArtifact]) -> String {
    let mut entries = artifacts
        .iter()
        .map(|artifact| {
            format!(
                "worktree\0{}\0{}\0{}\n",
                artifact.mapping.repository_id.as_deref().unwrap_or("root"),
                artifact.relative,
                artifact.sha256
            )
        })
        .collect::<Vec<_>>();
    entries.extend(branches.iter().map(|branch| {
        format!(
            "{}\0{}\0{}\0{}\0{}\n",
            branch.repository_id,
            branch.source_ref,
            branch.source_commit,
            branch.source_path,
            branch.sha256
        )
    }));
    entries.sort();
    format!("{:x}", Sha256::digest(entries.concat().as_bytes()))
}

pub(crate) fn legacy_workspace_id(artifacts: &[Artifact]) -> anyhow::Result<Option<uuid::Uuid>> {
    let Some(manifest) = artifacts
        .iter()
        .find(|artifact| artifact.relative == "manifest.json")
    else {
        return Ok(None);
    };
    legacy_workspace_id_from_manifest(&manifest.bytes)
}

pub(crate) fn legacy_workspace_id_from_manifest(
    bytes: &[u8],
) -> anyhow::Result<Option<uuid::Uuid>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| anyhow::anyhow!("legacy planning manifest is unreadable: {error}"))?;
    let manifest = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("legacy planning manifest must be a JSON object"))?;
    let schema_version = manifest
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("legacy planning manifest has no numeric schemaVersion"))?;
    anyhow::ensure!(
        (1..=4).contains(&schema_version),
        "unsupported legacy planning manifest schema {schema_version}"
    );
    anyhow::ensure!(
        manifest
            .get("product")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|product| !product.trim().is_empty()),
        "legacy planning manifest has no product name"
    );
    let parse = |key: &str| -> anyhow::Result<Option<uuid::Uuid>> {
        let Some(value) = value.get(key) else {
            return Ok(None);
        };
        let text = value.as_str().ok_or_else(|| {
            anyhow::anyhow!("legacy planning manifest {key} must be a UUID string")
        })?;
        Ok(Some(uuid::Uuid::parse_str(text).map_err(|error| {
            anyhow::anyhow!("legacy planning manifest {key} is not a valid UUID: {error}")
        })?))
    };
    let workspace_id = parse("workspaceId")?;
    let project_id = parse("projectId")?;
    anyhow::ensure!(
        workspace_id.is_none() || project_id.is_none() || workspace_id == project_id,
        "legacy planning manifest workspaceId and projectId disagree"
    );
    Ok(workspace_id.or(project_id))
}
