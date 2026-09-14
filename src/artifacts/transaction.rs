//! Recoverable multi-artifact planning write. A private git-common-dir journal
//! contains the pre-turn bytes until every file has reached its new version.
//! A crash with a journal rolls the incomplete turn back on next connection.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    path: String,
    before: Option<String>,
    after: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}

fn common(repo: &Path) -> anyhow::Result<PathBuf> {
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
    anyhow::ensure!(
        rel.starts_with("planning/") || rel == ".planner/workflow.json",
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
    Ok(common(repo)?.join("packet-planning-transaction.json"))
}
fn transaction_lock(repo: &Path) -> anyhow::Result<fs::File> {
    let lock = common(repo)?.join("packet-planning-transaction.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
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
                    fs::remove_file(target)?;
                }
            }
        }
    }
    Ok(())
}

pub fn recover(repo: &Path) -> anyhow::Result<bool> {
    let _lock = transaction_lock(repo)?;
    let journal = journal_path(repo)?;
    let text = match fs::read_to_string(&journal) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let state: Journal = serde_json::from_str(&text)?;
    restore(repo, &state.entries)?;
    fs::remove_file(journal)?;
    Ok(true)
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
    let staged = journal.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let serialized = serde_json::to_vec(&Journal {
        entries: entries.clone(),
    })?;
    let write = (|| -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        fs::rename(&staged, &journal)?;
        Ok(())
    })();
    if staged.exists() {
        let _ = fs::remove_file(&staged);
    }
    write?;
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
        fs::remove_file(journal)?;
        return Err(error);
    }
    fs::remove_file(journal)?;
    Ok(entries.into_iter().map(|e| e.path).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_document_set_restores_original_bytes() {
        let root = std::env::temp_dir().join(format!("packet_tx_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("planning/product")).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let a = root.join("planning/product/01-vision.md");
        let b = root.join("planning/product/02-scope.md");
        fs::write(&a, "old a").unwrap();
        fs::write(&b, "old b").unwrap();
        let changes = vec![
            ("planning/product/01-vision.md".into(), "new a".into()),
            ("planning/product/02-scope.md".into(), "new b".into()),
        ];
        assert!(apply_with_limit(&root, &changes, Some(1)).is_err());
        assert_eq!(fs::read_to_string(a).unwrap(), "old a");
        assert_eq!(fs::read_to_string(b).unwrap(), "old b");
        assert!(!journal_path(&root).unwrap().exists());
        assert_eq!(apply(&root, &changes).unwrap().len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
