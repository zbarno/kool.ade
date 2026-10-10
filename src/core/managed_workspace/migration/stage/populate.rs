mod io;
mod merge;
mod records;
mod sources;

pub(super) use sources::{SnapshotConflicts, current_artifacts, current_linked_worktree_artifacts};

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{planning_work::Work, state::PlannerState},
};
use sha2::Digest as _;

use super::super::{
    GeneratedArtifact, MigrationReport, WorkspaceManager,
    inventory::{Artifact, BranchArtifact, Kind},
};

pub(super) struct Snapshot {
    pub code_root: PathBuf,
    pub legacy_workspace_id: Option<uuid::Uuid>,
    pub state: PlannerState,
    pub work: Vec<Work>,
    pub open_items: Vec<crate::domain::OpenItem>,
    pub resolved_items: Vec<crate::domain::OpenItem>,
    pub artifacts: Vec<Artifact>,
    pub branches: Vec<BranchArtifact>,
    pub commit: String,
    pub git_ref: String,
    pub source_repositories: Vec<super::super::MigrationRepositorySource>,
    pub source_checkouts: Vec<SourceCheckout>,
}

#[derive(Debug, Clone)]
pub(super) struct SourceCheckout {
    pub repository_id: String,
    pub path: PathBuf,
    pub commit: String,
    pub git_ref: String,
    pub has_planning_root: bool,
    pub linked_worktree: bool,
}

pub(super) fn populate(
    manager: &WorkspaceManager,
    workspace_id: uuid::Uuid,
    stage: &Path,
    snapshot: &Snapshot,
) -> anyhow::Result<MigrationReport> {
    if !stage.exists() {
        fs::create_dir_all(stage)?;
        super::super::super::git::init_planning_repo(stage)?;
    }
    anyhow::ensure!(
        super::super::super::git::repository_root(stage)? == stage,
        "planning migration staging path contains an unrelated Git repository"
    );
    io::write_target(
        stage,
        "manifest.json",
        &serde_json::to_vec_pretty(&super::super::super::manifest::WorkspaceManifest::new(
            workspace_id,
        ))?,
    )?;
    io::write_gitignore(stage)?;

    for artifact in &snapshot.artifacts {
        match artifact.kind {
            Kind::Shared => io::write_target(
                stage,
                artifact.mapping.target_path.as_deref().unwrap(),
                &artifact.bytes,
            )?,
            Kind::Private => io::write_local(manager, workspace_id, artifact)?,
            Kind::Normalized | Kind::ReplacedManifest => {}
        }
    }

    let mut repositories = snapshot.state.repositories.clone();
    records::ensure_root_repository(&mut repositories, &snapshot.code_root)?;
    io::write_target(
        stage,
        crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
        &serde_json::to_vec_pretty(&repositories)?,
    )?;

    let store = PlanningStore::new(workspace_id, stage, StoreMode::ManagedLocal);
    crate::artifacts::migration::bootstrap_product_with_store(
        stage,
        &store,
        &snapshot.state.title,
    )?;
    let mut target_state = PlannerState::load_with_store(stage, &store)?;
    let _ = target_state.bootstrap_missing()?;
    records::write_normalized_records(&store, snapshot)?;
    crate::artifacts::task_docs::backfill_missing_task_records(&store, &snapshot.state.workflow)?;
    target_state = PlannerState::load_with_store(stage, &store)?;
    records::verify_typed_state(snapshot, &target_state, &store)?;

    let report = MigrationReport {
        schema_version: 1,
        workspace_id,
        source_commit: snapshot.commit.clone(),
        source_ref: snapshot.git_ref.clone(),
        source_repositories: snapshot.source_repositories.clone(),
        source_artifacts: snapshot
            .artifacts
            .iter()
            .map(|artifact| artifact.mapping.clone())
            .collect(),
        source_branches: snapshot.branches.clone(),
        generated_artifacts: generated_artifacts(stage, &snapshot.artifacts)?,
    };
    io::write_target(
        stage,
        "migration-report.json",
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn generated_artifacts(
    stage: &Path,
    source: &[Artifact],
) -> anyhow::Result<Vec<GeneratedArtifact>> {
    let source_targets = source
        .iter()
        .filter(|artifact| artifact.kind == Kind::Shared)
        .filter_map(|artifact| artifact.mapping.target_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut files = Vec::new();
    collect_target_files(stage, &mut files)?;
    let mut generated = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(stage)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "migration-report.json" || source_targets.contains(&relative) {
            continue;
        }
        generated.push(GeneratedArtifact {
            target_path: relative,
            sha256: format!("{:x}", sha2::Sha256::digest(fs::read(path)?)),
        });
    }
    generated.sort_by(|left, right| left.target_path.cmp(&right.target_path));
    Ok(generated)
}

fn collect_target_files(directory: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "planning target contains a symlink"
        );
        if metadata.is_dir() {
            collect_target_files(&path, out)?;
        } else {
            anyhow::ensure!(metadata.is_file(), "unsupported planning target artifact");
            out.push(path);
        }
    }
    Ok(())
}
