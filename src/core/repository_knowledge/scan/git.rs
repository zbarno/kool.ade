use std::{
    path::{Path, PathBuf},
    process::Command,
};

use super::super::source::{command_bytes_bounded, command_text, valid_oid};
use super::ProfileSource;

pub(super) fn normalize_ref(value: &str) -> Result<String, String> {
    let reference = if value.starts_with("refs/") {
        value.to_owned()
    } else {
        format!("refs/heads/{value}")
    };
    if !(reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/")) {
        return Err("repository source must be a branch ref".into());
    }
    let output = Command::new("git")
        .args(["check-ref-format", &reference])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env_remove("GIT_CONFIG_NOSYSTEM")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .output()
        .map_err(|_| "Git could not validate the selected repository ref".to_owned())?;
    if !output.status.success() {
        return Err("selected repository ref is invalid".into());
    }
    Ok(reference)
}

pub(super) fn resolve_root(source: &ProfileSource) -> Result<PathBuf, String> {
    let root = source
        .repositories
        .target_if_available(
            &source.planning_store,
            &source.code_root,
            &source.repository_id,
        )
        .map_err(|_| "selected code repository is unavailable".to_owned())?
        .ok_or_else(|| "selected code repository has no local checkout".to_owned())?;
    let canonical = root
        .canonicalize()
        .map_err(|_| "selected code repository cannot be opened".to_owned())?;
    if crate::core::gitops::worktree_root(&canonical).as_deref() != Some(canonical.as_path()) {
        return Err("selected path is not a Git worktree root".into());
    }
    Ok(canonical)
}

pub(super) fn resolve_commit(root: &Path, source_ref: &str) -> Result<String, String> {
    let output = command_text(
        root,
        &["rev-parse", "--verify", &format!("{source_ref}^{{commit}}")],
    )?;
    let commit = output.trim().to_owned();
    if !valid_oid(&commit) {
        return Err("Git returned an invalid repository commit ID".into());
    }
    Ok(commit)
}

pub(super) fn checkout_is_dirty(root: &Path) -> Result<bool, String> {
    let output = command_bytes_bounded(
        root,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
        1024 * 1024,
    );
    Ok(match output {
        Ok(bytes) => !bytes.is_empty(),
        Err(_) => true,
    })
}

pub(super) fn read_regular_blob(listing: &[u8], requested_path: &str) -> Result<String, String> {
    for record in listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        if &record[tab + 1..] != requested_path.as_bytes() {
            continue;
        }
        let metadata = String::from_utf8_lossy(&record[..tab]);
        let mut fields = metadata.split_ascii_whitespace();
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let oid = fields.next().unwrap_or_default();
        if !matches!(mode, "100644" | "100755") || kind != "blob" || !valid_oid(oid) {
            return Err("selected repository path is not a regular Git blob".into());
        }
        return Ok(oid.to_owned());
    }
    Err("selected repository file is missing at the pinned commit".into())
}
