//! Copy-verify-delete implementation for a fully preflighted migration plan.
use super::Plan;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

pub(super) fn apply(plan: &Plan, repo: &Path) -> anyhow::Result<()> {
    let mut destinations = BTreeMap::<String, &[u8]>::new();
    for entry in &plan.entries {
        destinations
            .entry(entry.target.clone())
            .or_insert(&entry.target_bytes);
    }
    for file in &plan.generated {
        destinations
            .entry(file.target.clone())
            .or_insert(&file.bytes);
    }
    for (relative, expected) in destinations {
        let target = repo.join(&relative);
        match fs::symlink_metadata(&target) {
            Ok(meta) => {
                anyhow::ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "Migration target {} is not a regular file",
                    relative
                );
                anyhow::ensure!(
                    fs::read(&target)? == expected,
                    "Migration conflict: {} changed after preflight; source files remain untouched",
                    relative
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_bytes(repo, &relative, expected)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    for entry in &plan.entries {
        let source = repo.join(&entry.source);
        let target = repo.join(&entry.target);
        anyhow::ensure!(
            fs::read(&source)? == entry.source_bytes,
            "Legacy artifact {} changed after preflight; it was preserved",
            entry.source
        );
        anyhow::ensure!(
            fs::read(&target)? == entry.target_bytes,
            "Migration target {} failed byte-for-byte verification; source was preserved",
            entry.target
        );
    }
    for entry in &plan.entries {
        fs::remove_file(repo.join(&entry.source))?;
        crate::artifacts::sync_parent_directory(&repo.join(&entry.source))?;
    }
    for root in [
        crate::artifacts::layout::legacy::PLANNING,
        crate::artifacts::layout::legacy::CONFIG,
        crate::artifacts::layout::legacy::ADR,
    ] {
        let path = repo.join(root);
        if path.is_dir() {
            remove_empty_tree(&path)?;
        }
    }
    Ok(())
}

fn write_bytes(repo: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let path = repo.join(relative);
    let parent = path.parent().expect("validated artifact path has a parent");
    create_directories(repo, parent)?;
    crate::artifacts::atomic_create_bytes(&path, bytes)?;
    Ok(())
}

fn create_directories(repo: &Path, parent: &Path) -> anyhow::Result<()> {
    let relative = parent.strip_prefix(repo)?;
    let mut path = repo.to_path_buf();
    for component in relative.components() {
        anyhow::ensure!(
            matches!(component, Component::Normal(_)),
            "Invalid migration directory"
        );
        path.push(component.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Migration directory {} is not a real directory",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&path)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn remove_empty_tree(path: &Path) -> anyhow::Result<()> {
    let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|child| child.file_name());
    for child in children {
        let child_path = child.path();
        let metadata = fs::symlink_metadata(&child_path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "Legacy artifact {} changed after migration preflight",
            child_path.display()
        );
        anyhow::ensure!(
            metadata.is_dir(),
            "Legacy artifact {} appeared after migration preflight; it was preserved",
            child_path.display()
        );
        remove_empty_tree(&child_path)?;
    }
    fs::remove_dir(path)?;
    Ok(())
}
