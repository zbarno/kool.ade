use super::Runner;
use std::{fs, path::Path, time::Duration};

pub(super) fn acquire(cache: &Path, runner: &Runner) -> anyhow::Result<fs::File> {
    let parent = cache
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Repository cache has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = cache
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Repository cache has no file name"))?;
    let lock_path = parent.join(format!("{}.lock", name.to_string_lossy()));
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                runner.remaining()?;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}
