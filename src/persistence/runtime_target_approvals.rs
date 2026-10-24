//! Per-operator approval evidence for custom runtime targets.
//!
//! Shared planning metadata is descriptive and can be edited by collaborators;
//! this local record proves that this operator approved the exact proposal.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod files;
mod origin;
#[cfg(test)]
mod tests;
use files::{check_private_file, ensure_private_dir, set_private_file};
pub(crate) use origin::{
    has_custom_target_origin, has_trusted_adapter_recipe_origin, save_custom_target_origin,
    save_discovered_target_origin,
};
pub(crate) use origin::{has_discovered_target_origin, save_trusted_adapter_recipe_origin};

const MAX_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    pub workspace_id: String,
    pub task_id: String,
    pub repository_id: String,
    pub source_sha: String,
    pub target_id: String,
    pub proposal_sha256: String,
    pub receipt_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u16,
    binding: Binding,
}

pub(crate) fn save(binding: &Binding) -> anyhow::Result<()> {
    validate(binding)?;
    let root = operator_approval_root()?;
    let record = Record {
        schema_version: 1,
        binding: binding.clone(),
    };
    let bytes = serde_json::to_vec(&record)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "runtime target approval is too large"
    );
    let legacy_path = legacy_record_path(&root, binding)?;
    if save_if_empty_or_matching(&legacy_path, binding, &bytes)? {
        return Ok(());
    }
    let binding_path = binding_record_path(&root, binding)?;
    anyhow::ensure!(
        save_if_empty_or_matching(&binding_path, binding, &bytes)?,
        "runtime target approval binding path conflicts with another record"
    );
    Ok(())
}

pub(crate) fn contains(binding: &Binding) -> anyhow::Result<bool> {
    validate(binding)?;
    let root = operator_approval_root()?;
    if read_binding(&legacy_record_path(&root, binding)?)?.as_ref() == Some(binding) {
        return Ok(true);
    }
    Ok(read_binding(&binding_record_path(&root, binding)?)?.as_ref() == Some(binding))
}

#[cfg(test)]
pub(crate) struct WorkspaceCleanup(String);

#[cfg(test)]
impl WorkspaceCleanup {
    pub(crate) fn new(workspace_id: &str) -> Self {
        Self(workspace_id.to_owned())
    }
}

#[cfg(test)]
impl Drop for WorkspaceCleanup {
    fn drop(&mut self) {
        let Ok(workspace) = uuid::Uuid::parse_str(&self.0) else {
            return;
        };
        let Ok(root) = operator_approval_root() else {
            return;
        };
        let Ok(canonical_root) = root.canonicalize() else {
            return;
        };
        let path = root
            .join("projects")
            .join(workspace.hyphenated().to_string());
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return;
        }
        let Ok(canonical) = path.canonicalize() else {
            return;
        };
        if canonical.starts_with(&canonical_root) {
            let _ = fs::remove_dir_all(canonical);
        }
    }
}

#[cfg(test)]
pub(crate) fn revoke(binding: &Binding) -> anyhow::Result<()> {
    validate(binding)?;
    let root = operator_approval_root()?;
    remove_matching_binding(&legacy_record_path(&root, binding)?, binding)?;
    remove_matching_binding(&binding_record_path(&root, binding)?, binding)
}

fn operator_approval_root() -> anyhow::Result<PathBuf> {
    operator_state_root_from(crate::persistence::state_root())
}

#[cfg(test)]
pub(super) fn operator_approval_root_from(
    home: Option<PathBuf>,
    user_profile: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    let root = home.or(user_profile).ok_or_else(|| {
        anyhow::anyhow!("operator home is unavailable; cannot persist runtime approval locally")
    })?;
    anyhow::ensure!(
        root.is_absolute(),
        "operator home is unavailable; cannot persist runtime approval locally"
    );
    let root = root.canonicalize().map_err(|error| {
        anyhow::anyhow!("operator home cannot be resolved for local runtime approval: {error}")
    })?;
    operator_state_root_from(root.join(".koolade-packet"))
}

fn operator_state_root_from(state_root: PathBuf) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        state_root.is_absolute(),
        "operator approval state root must be absolute"
    );
    anyhow::ensure!(
        !has_git_ancestor(&state_root),
        "operator approval state cannot be stored inside a Git repository"
    );
    fs::create_dir_all(&state_root)?;
    match fs::symlink_metadata(&state_root) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "operator approval state root must be a real directory"
            );
            let canonical = state_root.canonicalize()?;
            anyhow::ensure!(
                !has_git_ancestor(&canonical),
                "operator approval state cannot be stored inside a Git repository"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(state_root)
}

fn has_git_ancestor(path: &Path) -> bool {
    path.ancestors()
        .any(|ancestor| fs::symlink_metadata(ancestor.join(".git")).is_ok())
}

fn legacy_record_path(root: &Path, binding: &Binding) -> anyhow::Result<PathBuf> {
    let workspace = uuid::Uuid::parse_str(&binding.workspace_id)?;
    let receipt = uuid::Uuid::parse_str(&binding.receipt_id)?;
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let projects = root.join("projects");
    ensure_private_dir(&projects)?;
    let workspace_dir = projects.join(workspace.hyphenated().to_string());
    ensure_private_dir(&workspace_dir)?;
    let approvals = workspace_dir.join("runtime-target-approvals");
    ensure_private_dir(&approvals)?;
    anyhow::ensure!(
        approvals.canonicalize()?.starts_with(&root),
        "runtime target approval path escapes user state"
    );
    Ok(approvals.join(format!("{}.json", receipt.hyphenated())))
}

fn binding_record_path(root: &Path, binding: &Binding) -> anyhow::Result<PathBuf> {
    use sha2::Digest as _;

    let legacy_path = legacy_record_path(root, binding)?;
    let bytes = serde_json::to_vec(binding)?;
    let digest = sha2::Sha256::digest(bytes);
    let receipt = uuid::Uuid::parse_str(&binding.receipt_id)?;
    Ok(legacy_path.with_file_name(format!("{}-{:x}.json", receipt.hyphenated(), digest)))
}

fn save_if_empty_or_matching(path: &Path, binding: &Binding, bytes: &[u8]) -> anyhow::Result<bool> {
    if let Some(existing) = read_binding(path)? {
        return Ok(existing == *binding);
    }
    match crate::artifacts::atomic_create_bytes(path, bytes) {
        Ok(()) => {
            set_private_file(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(read_binding(path)?.as_ref() == Some(binding))
        }
        Err(error) => Err(error.into()),
    }
}

fn read_binding(path: &Path) -> anyhow::Result<Option<Binding>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "runtime target approval must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_BYTES,
        "runtime target approval exceeds the read limit"
    );
    check_private_file(path)?;
    let record: Record = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        record.schema_version == 1,
        "unsupported runtime target approval version"
    );
    Ok(Some(record.binding))
}

#[cfg(test)]
fn remove_matching_binding(path: &Path, binding: &Binding) -> anyhow::Result<()> {
    if read_binding(path)?.as_ref() != Some(binding) {
        return Ok(());
    }
    fs::remove_file(path)?;
    Ok(())
}

fn validate(binding: &Binding) -> anyhow::Result<()> {
    let is_hex = |value: &str, length| {
        value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    anyhow::ensure!(
        uuid::Uuid::parse_str(&binding.workspace_id).is_ok()
            && uuid::Uuid::parse_str(&binding.task_id).is_ok()
            && uuid::Uuid::parse_str(&binding.receipt_id).is_ok(),
        "invalid runtime target approval identity"
    );
    anyhow::ensure!(
        is_hex(&binding.source_sha, 40) && is_hex(&binding.proposal_sha256, 64),
        "invalid runtime target approval digest"
    );
    anyhow::ensure!(
        !binding.repository_id.is_empty()
            && binding.repository_id.len() <= 128
            && !binding.target_id.is_empty()
            && binding.target_id.len() <= 128,
        "invalid runtime target approval binding"
    );
    Ok(())
}
