use std::path::{Path, PathBuf};

use crate::core::managed_workspace::migration::inventory::{self, BranchArtifact, Kind};

use super::super::SourceCheckout;

pub(super) fn collect(
    repository_root: &Path,
    repository_id: &str,
) -> anyhow::Result<(Vec<SourceCheckout>, Vec<BranchArtifact>)> {
    let output = crate::core::managed_workspace::git::run(
        Some(repository_root),
        &["worktree", "list", "--porcelain", "-z"],
    )?;
    let current = repository_root.canonicalize()?;
    let mut checkouts = Vec::new();
    let mut artifacts = Vec::new();
    for field in output.stdout.split(|byte| *byte == 0) {
        let Some(path) = field.strip_prefix(b"worktree ") else {
            continue;
        };
        let path = PathBuf::from(std::str::from_utf8(path)?);
        let path = match path.canonicalize() {
            Ok(path) if path != current => path,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            crate::core::managed_workspace::repository_root(&path)? == path,
            "linked Git worktree path is not a repository root"
        );
        let commit = super::verification::git_text(&path, &["rev-parse", "HEAD"])?;
        let git_ref = super::verification::active_ref(&path)?;
        let source = inventory::collect_optional(&path, repository_id)?;
        let has_planning_root = path
            .join(crate::artifacts::layout::canonical::ROOT)
            .is_dir();
        for artifact in source {
            if artifact.kind == Kind::Private || artifact.relative == "manifest.json" {
                continue;
            }
            artifacts.push(BranchArtifact {
                repository_id: repository_id.to_owned(),
                source_ref: git_ref.clone(),
                source_commit: commit.clone(),
                source_path: artifact.mapping.source_path,
                sha256: artifact.sha256,
                disposition: "linked-worktree-snapshot".into(),
                record_ids: inventory::stable_record_ids(&artifact.relative, &artifact.bytes),
                target_path: artifact.mapping.target_path,
            });
        }
        checkouts.push(SourceCheckout {
            repository_id: repository_id.to_owned(),
            path,
            commit,
            git_ref,
            has_planning_root,
            linked_worktree: true,
        });
    }
    Ok((checkouts, artifacts))
}

pub(super) fn current_artifacts(checkout: &SourceCheckout) -> anyhow::Result<Vec<BranchArtifact>> {
    let current_commit = super::verification::git_text(&checkout.path, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        current_commit == checkout.commit,
        "linked worktree {} HEAD changed during migration",
        checkout.repository_id
    );
    let current_ref = super::verification::active_ref(&checkout.path)?;
    anyhow::ensure!(
        current_ref == checkout.git_ref,
        "linked worktree {} branch changed during migration",
        checkout.repository_id
    );
    let source = inventory::collect_optional(&checkout.path, &checkout.repository_id)?;
    let has_planning_root = checkout
        .path
        .join(crate::artifacts::layout::canonical::ROOT)
        .is_dir();
    anyhow::ensure!(
        has_planning_root == checkout.has_planning_root,
        "linked worktree planning root changed during migration"
    );
    Ok(source
        .into_iter()
        .filter(|artifact| artifact.kind != Kind::Private && artifact.relative != "manifest.json")
        .map(|artifact| BranchArtifact {
            repository_id: checkout.repository_id.clone(),
            source_ref: checkout.git_ref.clone(),
            source_commit: checkout.commit.clone(),
            source_path: artifact.mapping.source_path,
            sha256: artifact.sha256,
            disposition: "linked-worktree-snapshot".into(),
            record_ids: inventory::stable_record_ids(&artifact.relative, &artifact.bytes),
            target_path: artifact.mapping.target_path,
        })
        .collect())
}
