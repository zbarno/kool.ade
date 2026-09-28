use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use super::files::validate_tree;
use crate::{core::implementation::Implementation, harness::LiveProgress};

#[derive(Debug)]
pub(super) struct TargetMerge {
    source_files: BTreeMap<PathBuf, Vec<u8>>,
    target_files: BTreeMap<PathBuf, Vec<u8>>,
    activity: Option<ActivityMerge>,
}

#[derive(Debug)]
struct ActivityMerge {
    active: Vec<u8>,
    archived_path: PathBuf,
    archived: Vec<u8>,
}

pub(super) fn plan(
    source: &Path,
    target: &Path,
    expected: &Implementation,
) -> anyhow::Result<TargetMerge> {
    validate_tree(source)?;
    validate_tree(target)?;
    let source_files = files(source)?;
    let target_files = files(target)?;

    if let Some(bytes) = target_files.get(Path::new("state.json")) {
        let mut existing = crate::core::implementation::decode_state_bytes(bytes)?;
        existing.ticket = expected.ticket.clone();
        anyhow::ensure!(
            existing == *expected,
            "Implementation state at {} conflicts with {}; both copies were preserved",
            target.display(),
            source.display()
        );
    }

    let mut activity = None;
    for (relative, target_bytes) in &target_files {
        if relative == Path::new("state.json") {
            continue;
        }
        let Some(source_bytes) = source_files.get(relative) else {
            continue;
        };
        if source_bytes == target_bytes {
            continue;
        }
        anyhow::ensure!(
            relative == Path::new("activity.json"),
            "Conflicting implementation evidence at {} in {}; preserve both records and resolve the conflict before connecting",
            relative.display(),
            target.display()
        );
        activity = Some(plan_activity_merge(source_bytes, target_bytes)?);
    }
    if let Some(merge) = &activity {
        for files in [&source_files, &target_files] {
            if let Some(existing) = files.get(&merge.archived_path) {
                anyhow::ensure!(
                    existing == &merge.archived,
                    "Activity evidence archive {} already exists with different contents",
                    merge.archived_path.display()
                );
            }
        }
    }

    Ok(TargetMerge {
        source_files,
        target_files,
        activity,
    })
}

pub(super) fn apply(
    source: &Path,
    target: &Path,
    expected: &Implementation,
    plan: &TargetMerge,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        files(source)? == plan.source_files,
        "Implementation evidence at {} changed after migration preflight; both folders were preserved",
        source.display()
    );
    anyhow::ensure!(
        files(target)? == plan.target_files,
        "Implementation evidence at {} changed after migration preflight; both folders were preserved",
        target.display()
    );

    for (relative, bytes) in &plan.source_files {
        if relative == Path::new("state.json")
            || (relative == Path::new("activity.json") && plan.activity.is_some())
            || plan.target_files.contains_key(relative)
        {
            continue;
        }
        create_parent(target, relative)?;
        crate::artifacts::atomic_create_bytes(&target.join(relative), bytes)?;
    }

    if let Some(activity) = &plan.activity {
        let archived_path = target.join(&activity.archived_path);
        if archived_path.exists() {
            anyhow::ensure!(
                fs::read(&archived_path)? == activity.archived,
                "Preserved activity evidence {} conflicts with the migration snapshot",
                archived_path.display()
            );
        } else {
            crate::artifacts::atomic_create_bytes(&archived_path, &activity.archived)?;
        }
        crate::artifacts::atomic_write_bytes(&target.join("activity.json"), &activity.active)?;
    }

    let state_path = target.join("state.json");
    crate::artifacts::atomic_write_bytes(
        &state_path,
        &crate::core::implementation::serialize_state(expected)?,
    )?;
    let mut saved = crate::core::implementation::decode_state_bytes(&fs::read(&state_path)?)?;
    saved.ticket = expected.ticket.clone();
    anyhow::ensure!(
        saved == *expected,
        "Migrated implementation state failed verification"
    );
    Ok(())
}

fn plan_activity_merge(source: &[u8], target: &[u8]) -> anyhow::Result<ActivityMerge> {
    let source_progress: LiveProgress = serde_json::from_slice(source)
        .map_err(|error| anyhow::anyhow!("Cannot read source task activity: {error}"))?;
    let target_progress: LiveProgress = serde_json::from_slice(target)
        .map_err(|error| anyhow::anyhow!("Cannot read destination task activity: {error}"))?;
    let source_is_newer = source_progress.telemetry.updated_ms.unwrap_or_default()
        >= target_progress.telemetry.updated_ms.unwrap_or_default();
    let (active, archived) = if source_is_newer {
        (source.to_vec(), target.to_vec())
    } else {
        (target.to_vec(), source.to_vec())
    };
    let hash = crate::persistence::fnv1a64(&archived);
    let archived_path = PathBuf::from(format!("activity-migration-{hash:016x}.json"));
    anyhow::ensure!(
        archived_path != Path::new("activity.json"),
        "Activity archive path collides with the current task snapshot"
    );
    Ok(ActivityMerge {
        active,
        archived_path,
        archived,
    })
}

fn files(root: &Path) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut result = BTreeMap::new();
    collect(root, root, &mut result)?;
    Ok(result)
}

fn collect(
    root: &Path,
    directory: &Path,
    out: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "Implementation evidence {} is linked and cannot be merged safely",
            path.display()
        );
        if metadata.is_dir() {
            collect(root, &path, out)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "Unsupported implementation evidence {}",
                path.display()
            );
            out.insert(path.strip_prefix(root)?.to_path_buf(), fs::read(path)?);
        }
    }
    Ok(())
}

fn create_parent(root: &Path, relative: &Path) -> anyhow::Result<()> {
    let parent = relative
        .parent()
        .expect("implementation evidence has a parent");
    let mut current = root.to_path_buf();
    for component in parent.components() {
        anyhow::ensure!(
            matches!(component, Component::Normal(_)),
            "Invalid implementation evidence path {}",
            relative.display()
        );
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Implementation evidence parent {} is not a real directory",
                current.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
