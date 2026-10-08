use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use super::ensure_private_directory;

#[cfg(test)]
mod tests;

pub(in crate::harness::resource_bridge::npm) fn publish_index_snapshot(
    cache: &Path,
    snapshots: &Path,
) -> anyhow::Result<()> {
    const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_SNAPSHOT_STORAGE_BYTES: u64 = 512 * 1024 * 1024;
    let cache = cache.canonicalize()?;
    let cacache = cache.join("_cacache");
    ensure_private_directory(&cacache)?;
    let live_index = cacache.join("index-v5");
    ensure_private_directory(&live_index)?;
    let source = live_index.canonicalize()?;
    let snapshot_root = prepare_snapshot_root(snapshots)?;
    let used = directory_bytes(&snapshot_root)?;
    anyhow::ensure!(
        used < MAX_SNAPSHOT_STORAGE_BYTES,
        "Prepared npm index snapshots exceeded their per-run storage budget"
    );
    let generation = uuid::Uuid::new_v4().to_string();
    let snapshot = snapshot_root.join(&generation);
    let temporary_pointer = snapshot_root.join(format!(".current-{generation}"));
    let result = (|| {
        fs::create_dir(&snapshot)?;
        ensure_private_directory(&snapshot)?;
        let index = snapshot.join("index-v5");
        fs::create_dir(&index)?;
        ensure_private_directory(&index)?;
        let mut total = 0;
        copy_index_tree(
            &source,
            &index,
            &mut total,
            MAX_INDEX_BYTES.min(MAX_SNAPSHOT_STORAGE_BYTES - used),
        )?;
        let mut pointer = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary_pointer)?;
        writeln!(pointer, "{generation}")?;
        pointer.sync_all()?;
        fs::rename(&temporary_pointer, snapshot_root.join("current"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_pointer);
        let _ = fs::remove_dir_all(&snapshot);
    }
    result
}

fn prepare_snapshot_root(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Prepared npm snapshot root has no parent"))?
        .canonicalize()?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Prepared npm snapshot root has no name"))?;
    let root = parent.join(name);
    ensure_private_directory(&root)?;
    let canonical = root.canonicalize()?;
    anyhow::ensure!(
        canonical.starts_with(&parent),
        "Prepared npm snapshot root escaped its temporary owner"
    );
    Ok(canonical)
}

fn directory_bytes(root: &Path) -> anyhow::Result<u64> {
    let mut total = 0_u64;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        anyhow::ensure!(
            !kind.is_symlink(),
            "Prepared npm snapshot contains a symlink"
        );
        if kind.is_dir() {
            total = total.saturating_add(directory_bytes(&entry.path())?);
        } else {
            anyhow::ensure!(
                kind.is_file(),
                "Prepared npm snapshot contains a special file"
            );
            total = total.saturating_add(entry.metadata()?.len());
        }
    }
    Ok(total)
}

fn copy_index_tree(
    source: &Path,
    destination: &Path,
    total: &mut u64,
    limit: u64,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        anyhow::ensure!(!kind.is_symlink(), "Prepared npm index contains a symlink");
        if kind.is_dir() {
            fs::create_dir(&target)?;
            ensure_private_directory(&target)?;
            copy_index_tree(&entry.path(), &target, total, limit)?;
        } else {
            anyhow::ensure!(kind.is_file(), "Prepared npm index contains a special file");
            *total = total.saturating_add(entry.metadata()?.len());
            anyhow::ensure!(
                *total <= limit,
                "Prepared npm index exceeds its snapshot size limit"
            );
            fs::copy(entry.path(), &target)?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}
