//! Recoverable multi-artifact planning write. A private operator-local journal
//! scoped to the canonical planning-store root contains the pre-turn bytes
//! until every file has reached its new version.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod store;
pub(crate) use store::{
    apply_store, apply_store_with_removals, apply_store_with_removals_and_revision,
    apply_store_with_revision, recover_store, revision,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    path: String,
    before: Option<String>,
    after: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    root: String,
    entries: Vec<Entry>,
}

fn root_identity(repo: &Path) -> anyhow::Result<String> {
    Ok(repo.canonicalize()?.to_string_lossy().into_owned())
}

fn root_key(repo: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root_identity(repo)?.as_bytes());
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn common(repo: &Path) -> anyhow::Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot find git common directory for planning journal"
    );
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}
fn path(repo: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(rel);
    anyhow::ensure!(
        relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Invalid planning artifact path"
    );
    let canonical = crate::artifacts::layout::canonical::PLANNING;
    anyhow::ensure!(
        rel.starts_with(&format!("{canonical}/"))
            || rel == crate::artifacts::layout::canonical::WORKFLOW
            || rel == crate::artifacts::layout::canonical::WORK,
        "Path is outside planning artifacts"
    );
    let mut candidate = repo.to_path_buf();
    for part in relative.components() {
        candidate.push(part);
        if let Ok(meta) = fs::symlink_metadata(&candidate) {
            anyhow::ensure!(
                !meta.file_type().is_symlink(),
                "Symlink in planning artifact path"
            );
        }
    }
    Ok(candidate)
}
fn journal_path(repo: &Path) -> anyhow::Result<PathBuf> {
    Ok(common(repo)?.join(format!(
        "koolade-planning-transaction-{}.json",
        root_key(repo)?
    )))
}
pub(super) fn transaction_lock(repo: &Path) -> anyhow::Result<fs::File> {
    let lock = common(repo)?.join(format!(
        "koolade-planning-transaction-{}.lock",
        root_key(repo)?
    ));
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)?;
    file.lock()?;
    Ok(file)
}
fn restore(repo: &Path, entries: &[Entry]) -> anyhow::Result<()> {
    for entry in entries.iter().rev() {
        let target = path(repo, &entry.path)?;
        match &entry.before {
            Some(before) => crate::artifacts::atomic_write(&target, before)?,
            None => {
                if target.exists() {
                    fs::remove_file(&target)?;
                    crate::artifacts::sync_parent_directory(&target)?;
                }
            }
        }
    }
    Ok(())
}

pub fn recover(repo: &Path) -> anyhow::Result<bool> {
    crate::artifacts::migration::recover_transaction(repo)
}

pub fn apply(repo: &Path, changes: &[(String, String)]) -> anyhow::Result<Vec<String>> {
    apply_with_limit(repo, changes, None)
}
fn apply_with_limit(
    repo: &Path,
    changes: &[(String, String)],
    fail_after: Option<usize>,
) -> anyhow::Result<Vec<String>> {
    let _lock = transaction_lock(repo)?;
    anyhow::ensure!(
        !journal_path(repo)?.exists(),
        "Unrecovered planning transaction exists"
    );
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (rel, after) in changes {
        anyhow::ensure!(seen.insert(rel), "Duplicate planning artifact path {rel}");
        let target = path(repo, rel)?;
        let before = match fs::read_to_string(&target) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if before.as_deref() != Some(after) {
            entries.push(Entry {
                path: rel.clone(),
                before,
                after: after.clone(),
            });
        }
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let journal = journal_path(repo)?;
    let serialized = serde_json::to_vec(&Journal {
        root: root_identity(repo)?,
        entries: entries.clone(),
    })?;
    crate::artifacts::atomic_create_bytes(&journal, &serialized)?;
    let result = (|| -> anyhow::Result<()> {
        for (n, entry) in entries.iter().enumerate() {
            if fail_after == Some(n) {
                anyhow::bail!("injected interruption");
            }
            crate::artifacts::atomic_write(&path(repo, &entry.path)?, &entry.after)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        restore(repo, &entries)?;
        fs::remove_file(&journal)?;
        crate::artifacts::sync_parent_directory(&journal)?;
        return Err(error);
    }
    fs::remove_file(&journal)?;
    crate::artifacts::sync_parent_directory(&journal)?;
    Ok(entries.into_iter().map(|e| e.path).collect())
}

#[cfg(test)]
mod tests;
