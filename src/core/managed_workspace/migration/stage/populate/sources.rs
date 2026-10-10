use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use crate::{
    artifacts::planning_store::PlanningStore,
    core::{managed_workspace::migration::inventory, state::PlannerState},
};

use super::{Snapshot, SourceCheckout};

mod verification;
mod worktrees;

pub(crate) use verification::{current_artifacts, current_linked_worktree_artifacts};

pub(super) struct SourceData {
    pub checkout: SourceCheckout,
    pub artifacts: Vec<inventory::Artifact>,
    pub branches: Vec<inventory::BranchArtifact>,
    pub state: Option<PlannerState>,
    pub work: Vec<crate::core::planning_work::Work>,
    pub open_items: Vec<crate::domain::OpenItem>,
    pub resolved_items: Vec<crate::domain::OpenItem>,
}

#[derive(Debug)]
pub(crate) struct SnapshotConflicts(pub Vec<inventory::BranchConflict>);

impl std::fmt::Display for SnapshotConflicts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} planning sources contain divergent records or artifacts",
            self.0.len()
        )
    }
}

impl std::error::Error for SnapshotConflicts {}

impl Snapshot {
    pub fn read(code_root: PathBuf) -> anyhow::Result<Self> {
        read_sources(code_root, &BTreeMap::new())
    }

    pub fn read_with_local_checkouts(
        code_root: PathBuf,
        local_checkouts: &BTreeMap<String, PathBuf>,
    ) -> anyhow::Result<Self> {
        read_sources(code_root, local_checkouts)
    }
}

pub(super) fn read_sources(
    code_root: PathBuf,
    local_checkouts: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<Snapshot> {
    let code_root = code_root.canonicalize()?;
    let root_store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &code_root);
    let root_state = PlannerState::load_with_store(&code_root, &root_store)?;
    let mut repositories = root_state.repositories.clone();
    super::records::ensure_root_repository(&mut repositories, &code_root)?;

    let mut checkout_paths = BTreeMap::<String, PathBuf>::new();
    checkout_paths.insert("root".into(), code_root.clone());
    let mut source_summaries = Vec::new();
    let mut seen_paths = BTreeSet::new();
    seen_paths.insert(code_root.clone());
    for repository in &repositories.repositories {
        if repository.id == "root" {
            continue;
        }
        let candidate = match local_checkouts.get(&repository.id) {
            Some(path) => Some(path.clone()),
            None => repositories.target_if_available(&code_root, &repository.id)?,
        };
        if let Some(path) = candidate {
            let path = match super::super::super::canonical_code_checkout(&path) {
                Ok(path) => Some(path),
                Err(error) if verification::is_not_found(&error) => None,
                Err(error) => return Err(error),
            };
            let Some(path) = path else {
                continue;
            };
            anyhow::ensure!(
                seen_paths.insert(path.clone()),
                "registered code repositories resolve to the same checkout; resolve the repository identity mapping before migration"
            );
            checkout_paths.insert(repository.id.clone(), path);
        }
    }

    let mut sources = Vec::new();
    let mut source_checkouts = Vec::new();
    let mut ordered_repositories = repositories.repositories.iter().collect::<Vec<_>>();
    ordered_repositories
        .sort_by(|left, right| (left.id != "root", &left.id).cmp(&(right.id != "root", &right.id)));
    for repository in ordered_repositories {
        let Some(path) = checkout_paths.get(&repository.id).cloned() else {
            source_summaries.push(super::super::super::MigrationRepositorySource {
                repository_id: repository.id.clone(),
                source_commit: String::new(),
                source_ref: String::new(),
                artifact_count: 0,
                available: false,
            });
            continue;
        };
        let commit = verification::git_text(&path, &["rev-parse", "HEAD"])?;
        let git_ref = verification::active_ref(&path)?;
        let planning_root = path.join(crate::artifacts::layout::canonical::ROOT);
        let has_planning_root = match fs::symlink_metadata(&planning_root) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "legacy planning root must be a real directory: {}",
                    planning_root.display()
                );
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        let mut artifacts = if has_planning_root {
            inventory::collect(&path, &repository.id)?
        } else {
            Vec::new()
        };
        if repository.id != "root" {
            for artifact in &mut artifacts {
                if artifact.relative == "config/repositories.json" {
                    artifact.kind = inventory::Kind::ReplacedManifest;
                    artifact.mapping.disposition = "retained-root-workspace-registry".into();
                    artifact.mapping.target_path = None;
                }
            }
        }
        let branches = inventory::collect_branch_artifacts(&path, &repository.id, &git_ref)?;
        let (linked_checkouts, linked_branches) = worktrees::collect(&path, &repository.id)?;
        let mut branches = branches;
        branches.extend(linked_branches);
        let source_store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &path);
        let state = if has_planning_root {
            let state = PlannerState::load_with_store(&path, &source_store)?;
            let work = crate::core::planning_work::load(&source_store)?;
            super::records::audit_tasks(&source_store, &state)?;
            let (open_items, resolved_items, _) =
                crate::artifacts::items_io::load_store(&source_store)?;
            sources.push(SourceData {
                checkout: SourceCheckout {
                    repository_id: repository.id.clone(),
                    path: path.clone(),
                    commit: commit.clone(),
                    git_ref: git_ref.clone(),
                    has_planning_root,
                    linked_worktree: false,
                },
                artifacts: artifacts.clone(),
                branches: branches.clone(),
                state: Some(state.clone()),
                work,
                open_items,
                resolved_items,
            });
            Some(state)
        } else {
            sources.push(SourceData {
                checkout: SourceCheckout {
                    repository_id: repository.id.clone(),
                    path: path.clone(),
                    commit: commit.clone(),
                    git_ref: git_ref.clone(),
                    has_planning_root,
                    linked_worktree: false,
                },
                artifacts: Vec::new(),
                branches: branches.clone(),
                state: None,
                work: Vec::new(),
                open_items: Vec::new(),
                resolved_items: Vec::new(),
            });
            None
        };
        if repository.id == "root" {
            anyhow::ensure!(state.is_some(), "root planning source was not loaded");
        }
        source_summaries.push(super::super::super::MigrationRepositorySource {
            repository_id: repository.id.clone(),
            source_commit: commit.clone(),
            source_ref: git_ref.clone(),
            artifact_count: artifacts.len(),
            available: true,
        });
        source_checkouts.push(SourceCheckout {
            repository_id: repository.id.clone(),
            path,
            commit,
            git_ref,
            has_planning_root,
            linked_worktree: false,
        });
        source_checkouts.extend(linked_checkouts);
    }

    let mut conflicts = Vec::new();
    let mut workspace_ids = Vec::new();
    for source in &sources {
        if let Some(id) = inventory::legacy_workspace_id(&source.artifacts)? {
            workspace_ids.push((source, id));
        }
    }
    let distinct_workspace_ids = workspace_ids
        .iter()
        .map(|(_, id)| *id)
        .collect::<BTreeSet<_>>();
    if distinct_workspace_ids.len() > 1 {
        conflicts.push(inventory::BranchConflict {
            artifact_path: ".koolade-packet/manifest.json".into(),
            record_id: None,
            kind: "workspace-identity-divergence".into(),
            sources: workspace_ids
                .iter()
                .filter_map(|(source, id)| {
                    let artifact = source
                        .artifacts
                        .iter()
                        .find(|artifact| artifact.relative == "manifest.json")?;
                    Some(inventory::BranchArtifact {
                        repository_id: source.checkout.repository_id.clone(),
                        source_ref: source.checkout.git_ref.clone(),
                        source_commit: source.checkout.commit.clone(),
                        source_path: artifact.mapping.source_path.clone(),
                        sha256: artifact.sha256.clone(),
                        disposition: format!("workspace-id:{id}"),
                        record_ids: Vec::new(),
                        target_path: Some("manifest.json".into()),
                    })
                })
                .collect(),
        });
    }
    let legacy_workspace_id = workspace_ids
        .iter()
        .find(|(source, _)| source.checkout.repository_id == "root")
        .or_else(|| workspace_ids.first())
        .map(|(_, id)| *id);
    for source in &sources {
        conflicts.extend(inventory::branch_conflicts(
            &source.artifacts,
            &source.branches,
            &source.checkout.git_ref,
            &source.checkout.commit,
        ));
    }
    conflicts.extend(super::merge::artifact_conflicts(&sources));
    let merged = super::merge::merge(&sources);
    conflicts.extend(merged.conflicts);
    if !conflicts.is_empty() {
        return Err(anyhow::Error::new(SnapshotConflicts(conflicts)));
    }

    let mut root_state = root_state;
    root_state.repositories = repositories;
    root_state.workflow = merged.workflow;
    root_state.items = merged.open.clone();
    root_state.resolved_items = merged.resolved.clone();
    let artifacts = sources
        .iter()
        .flat_map(|source| source.artifacts.clone())
        .collect::<Vec<_>>();
    let branches = sources
        .iter()
        .flat_map(|source| source.branches.clone())
        .collect::<Vec<_>>();
    let root = sources
        .iter()
        .find(|source| source.checkout.repository_id == "root")
        .unwrap();
    Ok(Snapshot {
        code_root,
        legacy_workspace_id,
        state: root_state,
        work: merged.work,
        open_items: merged.open,
        resolved_items: merged.resolved,
        artifacts,
        branches,
        commit: root.checkout.commit.clone(),
        git_ref: root.checkout.git_ref.clone(),
        source_repositories: source_summaries,
        source_checkouts,
    })
}
