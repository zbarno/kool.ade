use std::path::{Path, PathBuf};

use super::{MAX_STATE_BYTES, MAX_TASKS, STORE_VERSION, Store, valid_id};

pub fn path() -> PathBuf {
    crate::persistence::state_root()
        .join("machine-setup")
        .join("tasks.json")
}

pub fn load() -> anyhow::Result<Store> {
    load_from(&path())
}

pub fn load_from(path: &Path) -> anyhow::Result<Store> {
    let bytes = match std::fs::metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.len() <= MAX_STATE_BYTES,
                "setup task state is oversized"
            );
            std::fs::read(path)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.into()),
    };
    let mut store: Store = serde_json::from_slice(&bytes)?;
    if store.schema_version == 1 {
        store.last_observed_at_unix_ms = store
            .tasks
            .values()
            .map(|task| task.observed_at_unix_ms)
            .max()
            .unwrap_or_default();
    }
    if matches!(store.schema_version, 1 | 2) {
        // Schema 1 is the committed public format. Schema 2 is retained as
        // best-effort compatibility for intermediate development stores.
        // Both used completion ordering, so migrate their watermark to scan
        // ordering and fail closed on ties.
        store.last_scan_started_at_unix_ms = store.last_observed_at_unix_ms;
        store.schema_version = STORE_VERSION;
    }
    anyhow::ensure!(
        store.schema_version == STORE_VERSION,
        "unsupported setup store schema"
    );
    anyhow::ensure!(store.tasks.len() <= MAX_TASKS, "oversized setup task store");
    for (key, task) in &store.tasks {
        anyhow::ensure!(
            valid_id(key) && key == &task.id,
            "invalid saved setup task ID"
        );
    }
    Ok(store)
}

pub fn save(store: &Store) -> anyhow::Result<()> {
    save_to(&path(), store)
}

pub fn save_to(path: &Path, store: &Store) -> anyhow::Result<()> {
    anyhow::ensure!(
        store.schema_version == STORE_VERSION,
        "unsupported setup store schema"
    );
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing setup state directory"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(parent) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "unsafe setup state directory"
        );
    }
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let payload = serde_json::to_string_pretty(store)?;
    anyhow::ensure!(
        payload.len() <= MAX_STATE_BYTES as usize,
        "setup task state is oversized"
    );
    crate::artifacts::atomic_write(path, &payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
