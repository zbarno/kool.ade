use super::{eligible, ignored_inventory, trusted};
use crate::core::implementation::{Implementation, Runner};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_BATCH: AtomicU64 = AtomicU64::new(0);

pub(in crate::core::implementation) fn quarantine_untrusted_ignored(
    runner: &Runner,
    state: &Implementation,
    artifact_dir: &Path,
) -> anyhow::Result<usize> {
    let trusted = trusted(runner, state, artifact_dir)?;
    let candidates = ignored_inventory(runner, &state.task_repository)?
        .into_iter()
        .filter(|path| eligible(path) && !trusted.contains(path))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(0);
    }

    let repository = state.task_repository.canonicalize()?;
    let quarantine_root = artifact_dir.join("generated-output-quarantine");
    fs::create_dir_all(&quarantine_root)?;
    let metadata = fs::symlink_metadata(&quarantine_root)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Generated-output quarantine is not a private directory"
    );
    set_private_directory(&quarantine_root)?;
    let quarantine_root = quarantine_root.canonicalize()?;
    let batch = create_batch(&quarantine_root)?;

    for relative in &candidates {
        let relative = safe_relative(relative)?;
        let source = safe_source(&repository, &relative)?;
        let destination = batch.join(&relative);
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Quarantine destination has no parent"))?;
        fs::create_dir_all(parent)?;
        anyhow::ensure!(
            parent.canonicalize()?.starts_with(&batch),
            "Quarantine destination escaped its private batch"
        );
        let source_metadata = fs::symlink_metadata(&source)?;
        anyhow::ensure!(
            source_metadata.is_file() || source_metadata.file_type().is_symlink(),
            "Ignored generated output is not a regular file or symlink"
        );
        move_entry(&source, &destination, &source_metadata)?;
        remove_empty_parents(&repository, source.parent());
    }
    Ok(candidates.len())
}

fn create_batch(root: &Path) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_default()
        .unsigned_abs();
    loop {
        let suffix = NEXT_BATCH.fetch_add(1, Ordering::Relaxed);
        let batch = root.join(format!("{stamp:016x}-{suffix:08x}"));
        match fs::create_dir(&batch) {
            Ok(()) => {
                if let Err(error) = set_private_directory(&batch) {
                    let _ = fs::remove_dir(&batch);
                    return Err(error);
                }
                return Ok(batch);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn safe_relative(path: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(path);
    anyhow::ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Ignored generated-output path is not a safe relative path"
    );
    Ok(relative.to_path_buf())
}

fn safe_source(repository: &Path, relative: &Path) -> anyhow::Result<PathBuf> {
    let source = repository.join(relative);
    let parent = source
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Ignored generated output has no parent"))?;
    anyhow::ensure!(
        parent.canonicalize()?.starts_with(repository),
        "Ignored generated output parent escapes the task repository"
    );
    let mut current = repository.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Ignored generated output traverses a non-directory or symlink"
        );
    }
    Ok(source)
}

fn move_entry(source: &Path, destination: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    anyhow::ensure!(
        fs::symlink_metadata(destination).is_err(),
        "Generated-output quarantine destination already exists"
    );
    if let Err(rename_error) = fs::rename(source, destination) {
        let copy_result = if metadata.file_type().is_symlink() {
            let target = fs::read_link(source)?;
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, destination)
            }
            #[cfg(not(unix))]
            {
                let _ = target;
                anyhow::bail!("Cannot safely preserve an ignored symlink on this platform");
            }
        } else {
            fs::copy(source, destination).map(|_| ())
        };
        copy_result.map_err(|copy_error| {
            anyhow::anyhow!(
                "Could not preserve ignored output {} (rename: {}; copy: {})",
                source.display(),
                rename_error,
                copy_error
            )
        })?;
        if !metadata.file_type().is_symlink() {
            fs::set_permissions(destination, metadata.permissions())?;
        }
        fs::remove_file(source)?;
    }
    Ok(())
}

fn remove_empty_parents(repository: &Path, mut parent: Option<&Path>) {
    while let Some(directory) = parent {
        if directory == repository || !directory.starts_with(repository) {
            break;
        }
        match fs::remove_dir(directory) {
            Ok(()) => parent = directory.parent(),
            Err(_) => break,
        }
    }
}
