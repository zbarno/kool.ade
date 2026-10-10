use std::path::Path;

use serde::Serialize;
use sha2::Digest as _;

use super::{
    super::{WorkspaceManager, inventory, journal::Journal},
    populate,
};

pub(super) fn verify_source_unchanged(snapshot: &populate::Snapshot) -> anyhow::Result<()> {
    let mut artifacts = Vec::new();
    let mut branches = Vec::new();
    for checkout in &snapshot.source_checkouts {
        anyhow::ensure!(
            git_text(&checkout.path, &["rev-parse", "HEAD"])? == checkout.commit,
            "registered code repository {} HEAD changed during migration",
            checkout.repository_id
        );
        anyhow::ensure!(
            active_ref(&checkout.path)? == checkout.git_ref,
            "registered code repository {} branch changed during migration",
            checkout.repository_id
        );
        if checkout.linked_worktree {
            branches.extend(populate::current_linked_worktree_artifacts(checkout)?);
        } else {
            let (source_artifacts, source_branches) = populate::current_artifacts(checkout)?;
            artifacts.extend(source_artifacts);
            branches.extend(source_branches);
        }
    }
    anyhow::ensure!(
        inventory::full_inventory_digest(&artifacts, &branches)
            == inventory::full_inventory_digest(&snapshot.artifacts, &snapshot.branches),
        "embedded planning artifacts or available branch snapshots changed during migration"
    );
    Ok(())
}

pub(super) fn git_text(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = super::super::super::git::run(Some(root), args)?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn active_ref(path: &Path) -> anyhow::Result<String> {
    let current_ref = super::super::super::git::output(
        Some(path),
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    Ok(if current_ref.status.success() {
        String::from_utf8_lossy(&current_ref.stdout)
            .trim()
            .to_owned()
    } else {
        "HEAD".into()
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictManifest<'a> {
    schema_version: u32,
    source_root: &'a Path,
    workspace_id: Option<uuid::Uuid>,
    inventory_sha256: Option<&'a str>,
    sources: Vec<ConflictSource>,
    conflicts: Vec<Conflict>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictSource {
    source_path: String,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Conflict {
    detail: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_id: Option<String>,
    sources: Vec<inventory::BranchArtifact>,
}

pub(super) fn write_conflict(
    manager: &WorkspaceManager,
    source_root: &Path,
    journal: Option<&Journal>,
    detail: &str,
    branch_conflicts: &[inventory::BranchConflict],
) -> anyhow::Result<()> {
    let journal_file =
        journal.map(|journal| super::super::journal::path(manager, &journal.source_root));
    let path = journal_file
        .map(|path| path.with_extension("conflicts.json"))
        .unwrap_or_else(|| {
            let digest = sha2::Sha256::digest(source_root.to_string_lossy().as_bytes());
            manager
                .home
                .join("projects/.migrations")
                .join(format!("{:x}.conflicts.json", digest))
        });
    let sources = inventory::collect(source_root, "root")
        .unwrap_or_default()
        .into_iter()
        .map(|artifact| ConflictSource {
            source_path: artifact.mapping.source_path,
            sha256: artifact.sha256,
        })
        .collect();
    let mut conflicts = branch_conflicts
        .iter()
        .map(|conflict| Conflict {
            detail: format!("{} for {}", conflict.kind, conflict.artifact_path),
            kind: conflict.kind.clone(),
            artifact_path: Some(conflict.artifact_path.clone()),
            record_id: conflict.record_id.clone(),
            sources: conflict.sources.clone(),
        })
        .collect::<Vec<_>>();
    if !detail.is_empty() {
        conflicts.push(Conflict {
            detail: detail.to_owned(),
            kind: "preflight-error".into(),
            artifact_path: None,
            record_id: None,
            sources: Vec::new(),
        });
    }
    let manifest = ConflictManifest {
        schema_version: 1,
        source_root,
        workspace_id: journal.map(|journal| journal.workspace_id),
        inventory_sha256: journal.map(|journal| journal.inventory_sha256.as_str()),
        sources,
        conflicts,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}
