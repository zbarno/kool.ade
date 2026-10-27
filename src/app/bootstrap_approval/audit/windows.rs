use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use fs2::FileExt;

use super::{AuditEntry, AuditStore, MAX_AUDIT_BYTES, MAX_EVENTS, VERSION};

mod storage;
use storage::{
    atomic_replace, open_directory, open_existing_audit, open_existing_file, open_new_file,
    open_or_create_file, verify_directory, verify_no_reparse_ancestors, verify_regular_file,
};

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn append(path: &Path, name: &str, entry: AuditEntry) -> anyhow::Result<()> {
    static PROCESS_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    let _process_guard = PROCESS_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("Repair audit lock was poisoned"))?;

    let directory_path = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit directory"))?;
    let state_root_path = directory_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit state root"))?;
    verify_no_reparse_ancestors(
        state_root_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid repair audit state root parent"))?,
    )?;
    let parent_path = state_root_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit state root parent"))?;
    let parent = open_directory(parent_path)?;
    verify_directory(parent_path, &parent)?;
    match fs::create_dir(state_root_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    verify_no_reparse_ancestors(state_root_path)?;
    let state_root = open_directory(state_root_path)?;
    verify_directory(parent_path, &parent)?;
    verify_directory(state_root_path, &state_root)?;

    match fs::create_dir(directory_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let directory = open_directory(directory_path)?;
    verify_no_reparse_ancestors(directory_path)?;
    verify_directory(directory_path, &directory)?;

    let lock_path = directory_path.join(format!("{name}.lock"));
    let lock = open_or_create_file(&lock_path)?;
    verify_regular_file(&lock_path, &lock)?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Another application instance is recording a machine repair"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    verify_directory(state_root_path, &state_root)?;
    verify_directory(parent_path, &parent)?;
    verify_directory(directory_path, &directory)?;
    verify_regular_file(&lock_path, &lock)?;

    let mut store = match open_existing_audit(path)? {
        Some(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_AUDIT_BYTES + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_AUDIT_BYTES,
                "Machine repair audit is oversized"
            );
            let store: AuditStore = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(
                store.schema_version == VERSION && store.entries.len() <= MAX_EVENTS,
                "Machine repair audit has an unsupported schema or size"
            );
            store
        }
        None => AuditStore {
            schema_version: VERSION,
            entries: Vec::new(),
        },
    };
    store.entries.push(entry);
    if store.entries.len() > MAX_EVENTS {
        store.entries.drain(..store.entries.len() - MAX_EVENTS);
    }
    let bytes = serde_json::to_vec(&store)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_AUDIT_BYTES,
        "Machine repair audit is oversized"
    );

    let temporary_path = directory_path.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut temporary = open_new_file(&temporary_path)?;
    let write_result = (|| -> anyhow::Result<()> {
        verify_regular_file(&temporary_path, &temporary)?;
        temporary.write_all(&bytes)?;
        temporary.sync_all()?;
        verify_directory(parent_path, &parent)?;
        verify_directory(state_root_path, &state_root)?;
        verify_directory(directory_path, &directory)?;
        verify_regular_file(&lock_path, &lock)?;
        atomic_replace(&temporary_path, path)?;
        verify_regular_file(path, &open_existing_file(path)?)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

#[cfg(test)]
mod tests;
