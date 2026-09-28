//! One-time, restartable migration of shared repository artifacts.
mod identities;
mod plan;
mod product;
mod state;
mod transaction;

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(test)]
mod tests;

const SCHEMA_VERSION: u32 = 4;
const PENDING_NAME: &str = "packet-artifact-migration.pending.json";
const LOCK_NAME: &str = "packet-artifact-migration.lock";

/// Roll back a pre-migration planning journal before legacy files are moved.
/// This is the only startup path that is allowed to write to legacy locations.
pub(crate) fn recover_transaction(repo: &Path) -> anyhow::Result<bool> {
    transaction::recover(repo)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    product: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct Pending {
    source_schema_version: u32,
    paths: BTreeSet<String>,
}

/// Migrate before loading planner state. The marker lives in Git's common
/// directory so a process restart can finish the checkpoint after file moves.
pub fn run(repo: &Path) -> anyhow::Result<Vec<String>> {
    let repo = repo.canonicalize()?;
    let _writer = crate::core::writer_gate::acquire();
    let common = common_dir(&repo)?;
    let _lock = lock(&common)?;
    let layout = crate::artifacts::layout::ArtifactLayout::new(&repo);
    let manifest_path = layout.manifest();
    let pending_path = common.join(PENDING_NAME);

    let manifest = read_manifest(&manifest_path)?;
    if let Some(manifest) = &manifest {
        anyhow::ensure!(
            manifest.product == "Packet",
            "Artifact manifest belongs to a different product"
        );
        anyhow::ensure!(
            manifest.schema_version <= SCHEMA_VERSION,
            "Artifact schema {} is newer than this Packet build supports",
            manifest.schema_version
        );
    }
    let source_schema_version = manifest
        .as_ref()
        .map(|manifest| manifest.schema_version)
        .unwrap_or_else(|| detect_source_schema(&repo));
    let plan = plan::Plan::build(&repo)?;
    let identity_plan = identities::Plan::build(&repo, &plan)?;
    let private = state::PrivatePlan::build(&repo, &common, &plan, identity_plan.task_uids())?;
    let pending_exists = pending_path.exists();
    if manifest
        .as_ref()
        .is_some_and(|m| m.schema_version == SCHEMA_VERSION)
        && !pending_exists
        && plan.is_empty()
        && identity_plan.is_empty()
        && private.is_empty()
    {
        return Ok(Vec::new());
    }

    let mut pending = read_pending(&pending_path)?;
    if pending.source_schema_version == 0 {
        pending.source_schema_version = source_schema_version;
    }
    pending.paths.extend(plan.git_paths());
    pending.paths.extend(identity_plan.git_paths());
    pending
        .paths
        .insert(crate::artifacts::layout::canonical::MANIFEST.into());
    write_pending(&pending_path, &pending)?;

    plan.apply(&repo)?;
    identity_plan.apply(&repo)?;
    private.apply()?;
    let workflow_path = layout.workflow_state();
    let before_workflow = match fs::read_to_string(&workflow_path) {
        Ok(text) => serde_json::from_str::<crate::core::workflow::Workflow>(&text)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::core::workflow::Workflow::default()
        }
        Err(error) => return Err(error.into()),
    };
    let workflow = crate::artifacts::task_docs::load_workflow(&repo)?;
    let mut migrated_features = Vec::new();
    for (feature_id, _) in crate::artifacts::product_docs::active_features(&repo) {
        let path =
            crate::artifacts::product_docs::document_path(&repo, &format!("feature:{feature_id}"))?;
        let markdown = fs::read_to_string(&path)?;
        let updated = crate::domain::ChangeMetadata::clear_legacy_comparison_state(&markdown)?;
        if updated != markdown {
            migrated_features.push((path, updated));
        }
    }
    if workflow != before_workflow || !migrated_features.is_empty() {
        if workflow != before_workflow {
            pending.paths.insert(
                workflow_path
                    .strip_prefix(&repo)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        for (path, _) in &migrated_features {
            pending.paths.insert(
                path.strip_prefix(&repo)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        write_pending(&pending_path, &pending)?;
        if workflow != before_workflow {
            crate::artifacts::task_docs::save_workflow(&repo, &workflow)?;
        }
        for (path, markdown) in migrated_features {
            crate::artifacts::atomic_write(&path, &markdown)?;
        }
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        product: "Packet".into(),
    };
    crate::artifacts::atomic_write(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;

    let paths = pending.paths.into_iter().collect::<Vec<_>>();
    checkpoint(&repo, &paths)?;
    fs::remove_file(&pending_path)?;
    crate::artifacts::sync_parent_directory(&pending_path)?;
    Ok(paths)
}

/// Bootstrap product modules for callers that create state directly instead
/// of entering through the connection pipeline. Normal connection uses
/// `run`, which plans, versions, and checkpoints the same files with every
/// other project artifact.
pub(crate) fn bootstrap_product(repo: &Path, title: &str) -> anyhow::Result<Vec<String>> {
    let generated = product::bootstrap_files(repo, Some(title))?;
    let mut paths = Vec::with_capacity(generated.len());
    for file in generated {
        let text = String::from_utf8(file.bytes)?;
        crate::artifacts::atomic_write(&repo.join(&file.target), &text)?;
        paths.push(file.target);
    }
    Ok(paths)
}

#[cfg(test)]
pub(crate) fn migrate_files_for_test(repo: &Path) -> anyhow::Result<Vec<String>> {
    let plan = plan::Plan::build(repo)?;
    let paths = plan.git_paths();
    plan.apply(repo)?;
    Ok(paths)
}

fn read_manifest(path: &Path) -> anyhow::Result<Option<Manifest>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Unversioned projects are source schema 1 when either the old layout or a
/// partially-created Packet root exists; an untouched repository is schema 0.
fn detect_source_schema(repo: &Path) -> u32 {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    let has_legacy = [
        crate::artifacts::layout::legacy::PLANNING,
        crate::artifacts::layout::legacy::CONFIG,
        crate::artifacts::layout::legacy::ADR,
        crate::artifacts::layout::legacy::ROOT_SPECIFICATION,
    ]
    .iter()
    .any(|path| repo.join(path).exists());
    u32::from(has_legacy || layout.packet_root().exists())
}

fn read_pending(path: &Path) -> anyhow::Result<Pending> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Pending::default()),
        Err(error) => Err(error.into()),
    }
}

fn write_pending(path: &Path, pending: &Pending) -> anyhow::Result<()> {
    crate::artifacts::atomic_write(path, &serde_json::to_string_pretty(pending)?)
}

fn common_dir(repo: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot locate Git common directory for artifact migration"
    );
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

/// Coordinate first creation of task evidence with path migration across
/// Packet instances. Workers release this after owning the per-task lock.
pub(crate) fn acquire_project_state_gate(repo: &Path) -> anyhow::Result<fs::File> {
    lock(&common_dir(repo)?)
}

fn lock(common: &Path) -> anyhow::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common.join(LOCK_NAME))?;
    file.lock()?;
    Ok(file)
}

fn checkpoint(repo: &Path, paths: &[String]) -> anyhow::Result<()> {
    let paths = paths
        .iter()
        .filter(|path| repo.join(path).exists() || crate::core::gitops::tracked_in_head(repo, path))
        .cloned()
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(());
    }
    crate::core::gitops::commit(repo, "packet: migrate project artifacts", &paths).map_err(
        |error| {
            anyhow::anyhow!(
                "Migrated artifacts are preserved, but the migration checkpoint failed: {error}"
            )
        },
    )?;
    Ok(())
}
