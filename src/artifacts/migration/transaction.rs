//! Roll back old or current planning journals before artifact migration.
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
struct Entry {
    path: String,
    before: Option<String>,
}

#[derive(Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}

fn common_dir(repo: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot find git common directory for planning journal"
    );
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn journal_path(repo: &Path) -> anyhow::Result<PathBuf> {
    Ok(common_dir(repo)?.join("koolade-planning-transaction.json"))
}

fn lock(repo: &Path) -> anyhow::Result<fs::File> {
    let path = common_dir(repo)?.join("koolade-planning-transaction.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

fn recovery_path(repo: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(relative);
    anyhow::ensure!(
        path.components()
            .all(|component| matches!(component, std::path::Component::Normal(_))),
        "Invalid planning artifact path in recovery journal"
    );
    let canonical = crate::artifacts::layout::canonical::PLANNING;
    let legacy = crate::artifacts::layout::legacy::PLANNING;
    anyhow::ensure!(
        relative.starts_with(&format!("{canonical}/"))
            || relative == crate::artifacts::layout::canonical::WORKFLOW
            || relative.starts_with(&format!("{legacy}/"))
            || relative == crate::artifacts::layout::legacy::WORKFLOW,
        "Recovery journal path is outside planning artifacts"
    );
    let mut target = repo.to_path_buf();
    for component in path.components() {
        target.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Symlink in planning artifact recovery path"
            );
        }
    }
    Ok(target)
}

pub(super) fn recover(repo: &Path) -> anyhow::Result<bool> {
    let _lock = lock(repo)?;
    let journal = journal_path(repo)?;
    let text = match fs::read_to_string(&journal) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let state: Journal = serde_json::from_str(&text)?;
    for entry in state.entries.iter().rev() {
        let target = recovery_path(repo, &entry.path)?;
        match &entry.before {
            Some(before) => crate::artifacts::atomic_write(&target, before)?,
            None if target.exists() => {
                fs::remove_file(&target)?;
                crate::artifacts::sync_parent_directory(&target)?;
            }
            None => {}
        }
    }
    fs::remove_file(&journal)?;
    crate::artifacts::sync_parent_directory(&journal)?;
    Ok(true)
}
