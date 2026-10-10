use std::{fs, path::Path};

pub(super) struct WorkspaceLock {
    _file: fs::File,
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

pub(super) fn lock(path: &Path) -> anyhow::Result<WorkspaceLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()
        .map_err(|_| anyhow::anyhow!("workspace directory is locked"))?;
    Ok(WorkspaceLock { _file: file })
}
