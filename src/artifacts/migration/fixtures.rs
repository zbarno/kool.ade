//! Test fixture helper for upgrading an old specification inside a planning
//! repository. Production setup never migrates or searches code checkouts.

use std::fs;
use std::path::Path;

use crate::artifacts::planning_store::{PlanningLayout, PlanningStore};

/// Normalizes a planning repository fixture and returns changed paths.
pub fn migrate_files_for_test(repo: &Path) -> anyhow::Result<Vec<String>> {
    let store = PlanningStore::new(
        uuid::Uuid::nil(),
        repo,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let layout = store.layout();
    let mut paths = crate::artifacts::migration::bootstrap_product_from_seed(repo, &store)?;

    ensure_root_manifest(repo, &mut paths)?;
    archive_legacy_specification(repo, &layout, &mut paths)?;
    Ok(paths)
}

/// Planning-repository metadata used by test stores. The old product-only
/// root manifest was incompatible with managed planning repositories.
fn ensure_root_manifest(repo: &Path, paths: &mut Vec<String>) -> anyhow::Result<()> {
    let manifest = repo.join(crate::artifacts::layout::store::MANIFEST);
    let branch = std::process::Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "main".to_owned());
    if manifest.is_file()
        && std::fs::read(&manifest)
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<crate::core::managed_workspace::WorkspaceManifest>(&bytes)
                    .ok()
            })
            .is_some_and(|current| current.kind == "koolade-planning")
    {
        return Ok(());
    }
    let mut workspace =
        crate::core::managed_workspace::WorkspaceManifest::new(uuid::Uuid::new_v4());
    workspace.canonical_ref = format!("refs/heads/{branch}");
    fs::create_dir_all(manifest.parent().unwrap())?;
    fs::write(&manifest, serde_json::to_vec_pretty(&workspace)?)?;
    paths.push(crate::artifacts::layout::store::MANIFEST.to_owned());
    Ok(())
}

fn archive_legacy_specification(
    repo: &Path,
    layout: &PlanningLayout,
    paths: &mut Vec<String>,
) -> anyhow::Result<()> {
    let spec = layout.legacy_specification();
    let archive = repo.join(crate::artifacts::layout::store::LEGACY_SPEC_ARCHIVE);
    let spec_present = spec.try_exists()?;
    let archive_taken = archive.try_exists()?;

    if spec_present && archive_taken {
        anyhow::bail!(
            "legacy specification cannot be archived: {} already holds other evidence",
            archive.display()
        );
    }
    if spec_present {
        fs::create_dir_all(archive.parent().unwrap())?;
        fs::rename(&spec, &archive)?;
        paths.push(crate::artifacts::layout::store::LEGACY_SPEC_ARCHIVE.to_owned());
    }
    Ok(())
}
