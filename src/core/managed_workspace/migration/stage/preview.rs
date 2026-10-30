use std::path::Path;

use super::super::{
    MigrationConflictPreview, MigrationPreview, canonical_code_checkout, inventory,
};
use super::populate::Snapshot;

pub fn preview(code_checkout: &Path) -> anyhow::Result<MigrationPreview> {
    let root = canonical_code_checkout(code_checkout)?;
    let snapshot = Snapshot::read(root.clone())?;
    Ok(from_snapshot(&snapshot))
}

pub(super) fn from_snapshot(snapshot: &Snapshot) -> MigrationPreview {
    let collisions = inventory::branch_conflicts(
        &snapshot.artifacts,
        &snapshot.branches,
        &snapshot.git_ref,
        &snapshot.commit,
    )
    .into_iter()
    .map(|conflict| MigrationConflictPreview {
        path: conflict.artifact_path,
        record_id: conflict.record_id,
        kind: conflict.kind,
    })
    .collect();
    let mut record_ids = snapshot
        .work
        .iter()
        .map(|work| work.uid.clone())
        .chain(snapshot.open_items.iter().map(|item| item.id.clone()))
        .chain(snapshot.resolved_items.iter().map(|item| item.id.clone()))
        .collect::<Vec<_>>();
    record_ids.sort();
    record_ids.dedup();
    let exclusions = snapshot
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == inventory::Kind::Private)
        .map(|artifact| artifact.mapping.source_path.clone())
        .collect();
    MigrationPreview {
        source_root: snapshot.code_root.clone(),
        workspace_id: snapshot.legacy_workspace_id,
        source_commit: snapshot.commit.clone(),
        source_ref: snapshot.git_ref.clone(),
        inventory_digest: inventory::full_inventory_digest(&snapshot.artifacts, &snapshot.branches),
        file_count: snapshot.artifacts.len(),
        record_ids,
        collisions,
        backup_path: snapshot
            .code_root
            .join(crate::artifacts::layout::store::ROOT),
        exclusions,
    }
}
