//! Cross-process coordination gates for private project state.
use std::{fs, path::PathBuf};

fn lock_root() -> PathBuf {
    crate::persistence::state_root().join("locks")
}

fn root_identity(repo: &std::path::Path) -> anyhow::Result<String> {
    Ok(repo.canonicalize()?.to_string_lossy().into_owned())
}

fn root_key(repo: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root_identity(repo)?.as_bytes());
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn lock_path(root: &std::path::Path, repo: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    Ok(root.join(format!("project-state-{}.lock", root_key(repo)?)))
}

fn lock(root: &std::path::Path, repo: &std::path::Path) -> anyhow::Result<fs::File> {
    let path = lock_path(root, repo)?;
    fs::create_dir_all(root)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

/// Coordinate private state transitions across app instances for one code repo.
pub(crate) fn acquire_project_state_gate(repo: &std::path::Path) -> anyhow::Result<fs::File> {
    lock(&lock_root(), repo)
}

#[cfg(test)]
#[path = "locks_tests.rs"]
mod tests;
