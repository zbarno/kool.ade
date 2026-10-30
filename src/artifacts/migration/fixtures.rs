//! Test-suite stand-in for the retired in-checkout artifact migration.
//!
//! Brings a schema-1 fixture corpus (a bare `planning/` directory beside the
//! source) into the current packet-internal layout: bootstraps product
//! modules from the legacy specification, archives the source specification
//! with collision protection, and relocates the remaining planning
//! documents. It deliberately omits the versioned checkpoint and journal
//! machinery, which no longer exists in production code paths.

use std::fs;
use std::path::{Path, PathBuf};

use crate::artifacts::layout::legacy;
use crate::artifacts::planning_store::{PlanningLayout, PlanningStore};

/// Migrates a legacy fixture checkout and returns the checkout-relative
/// paths that were created or relocated.
pub fn migrate_files_for_test(repo: &Path) -> anyhow::Result<Vec<String>> {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
    let layout = store.layout();
    let mut paths = crate::artifacts::migration::bootstrap_product_from_seed(repo, &store)?;

    ensure_root_manifest(repo, &mut paths)?;
    archive_legacy_specification(repo, &layout, &mut paths)?;
    relocate_remaining_planning(&store, &layout, &mut paths)?;
    Ok(paths)
}

/// Packet recognition manifest, matching the production bootstrap payload.
fn ensure_root_manifest(repo: &Path, paths: &mut Vec<String>) -> anyhow::Result<()> {
    let manifest = repo.join(crate::artifacts::layout::store::MANIFEST);
    if manifest.try_exists()? {
        return Ok(());
    }
    fs::create_dir_all(manifest.parent().unwrap())?;
    fs::write(
        &manifest,
        b"{\n  \"schemaVersion\": 4,\n  \"product\": \"Koolade\"\n}\n",
    )?;
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
    let stale = repo.join(legacy::SPEC_ARCHIVE);
    let spec_present = spec.try_exists()?;
    let archive_taken = archive.try_exists()?;
    let stale_present = stale.try_exists()?;

    if spec_present && (archive_taken || stale_present) {
        anyhow::bail!(
            "legacy specification cannot be archived: {} already holds other evidence",
            archive.display()
        );
    }
    if !spec_present && stale_present && archive_taken {
        anyhow::bail!(
            "archived legacy specification cannot be restored: {} already holds other evidence",
            archive.display()
        );
    }
    if stale_present {
        fs::create_dir_all(archive.parent().unwrap())?;
        fs::rename(&stale, &archive)?;
        paths.push(legacy::SPEC_ARCHIVE.to_owned());
        paths.push(crate::artifacts::layout::store::LEGACY_SPEC_ARCHIVE.to_owned());
    }
    if spec_present {
        fs::create_dir_all(archive.parent().unwrap())?;
        fs::rename(&spec, &archive)?;
        paths.push(crate::artifacts::layout::store::LEGACY_SPEC_ARCHIVE.to_owned());
    }
    Ok(())
}

fn relocate_remaining_planning(
    store: &PlanningStore,
    layout: &PlanningLayout,
    paths: &mut Vec<String>,
) -> anyhow::Result<()> {
    let source = layout.legacy_planning_root();
    if !source.is_dir() {
        return Ok(());
    }
    let dest_base = store.root.join("planning");
    let mut files = Vec::new();
    collect_files(&source, &mut files)?;
    for file in &files {
        let relative = file.strip_prefix(&source)?;
        let top_level = relative
            .components()
            .next()
            .map(|component| component.as_os_str().to_os_string())
            .unwrap_or_default();
        if top_level == "product" || top_level == "archive" {
            continue; // Owned by product bootstrap and spec archiving.
        }
        if relative.as_os_str() == "specification.md" {
            continue;
        }
        let dest = dest_base.join(relative);
        if dest.exists() {
            let same = matches!(
                (fs::read(&dest), fs::read(file)),
                (Ok(existing), Ok(candidate)) if existing == candidate
            );
            anyhow::ensure!(
                same,
                "cannot relocate {}: destination already differs",
                relative.display()
            );
            fs::remove_file(file)?;
            continue;
        }
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::rename(file, &dest)?;
        paths.push(format!(
            "{}/{}",
            crate::artifacts::layout::store::PLANNING,
            relative.to_string_lossy()
        ));
    }
    prune_empty_directories(&source);
    Ok(())
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn prune_empty_directories(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_empty_directories(&path);
        }
    }
    let _ = fs::remove_dir(dir);
}
