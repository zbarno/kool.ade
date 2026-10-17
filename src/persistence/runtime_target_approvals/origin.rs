use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TargetOriginKind {
    CustomApproval,
    DiscoveredTarget,
    TrustedAdapterRecipe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetOrigin {
    schema_version: u16,
    kind: TargetOriginKind,
    workspace_id: String,
    task_id: String,
    origin_sha256: String,
}

pub(crate) fn save_custom_target_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<()> {
    save_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::CustomApproval,
    )
}

pub(crate) fn has_custom_target_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<bool> {
    has_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::CustomApproval,
    )
}

pub(crate) fn save_discovered_target_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<()> {
    save_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::DiscoveredTarget,
    )
}

#[cfg(test)]
pub(crate) fn has_discovered_target_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<bool> {
    has_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::DiscoveredTarget,
    )
}

#[cfg(test)]
pub(crate) fn save_trusted_adapter_recipe_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<()> {
    save_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::TrustedAdapterRecipe,
    )
}

pub(crate) fn has_trusted_adapter_recipe_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<bool> {
    has_origin(
        workspace_id,
        task_id,
        origin_sha256,
        TargetOriginKind::TrustedAdapterRecipe,
    )
}

fn save_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
    kind: TargetOriginKind,
) -> anyhow::Result<()> {
    super::files::validate_origin(workspace_id, task_id, origin_sha256)?;
    let record = TargetOrigin {
        schema_version: 1,
        kind,
        workspace_id: workspace_id.to_owned(),
        task_id: task_id.to_owned(),
        origin_sha256: origin_sha256.to_owned(),
    };
    let path = origin_path(&record)?;
    let bytes = serde_json::to_vec(&record)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "runtime target origin must be a regular file"
            );
            anyhow::ensure!(
                metadata.len() <= super::MAX_BYTES,
                "runtime target origin exceeds the read limit"
            );
            super::files::check_private_file(&path)?;
            anyhow::ensure!(
                read_origin(&path)? == record,
                "runtime target origin conflicts with existing local provenance"
            );
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    crate::artifacts::atomic_create_bytes(&path, &bytes)?;
    super::files::set_private_file(&path)?;
    Ok(())
}

fn has_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
    kind: TargetOriginKind,
) -> anyhow::Result<bool> {
    super::files::validate_origin(workspace_id, task_id, origin_sha256)?;
    let record = TargetOrigin {
        schema_version: 1,
        kind,
        workspace_id: workspace_id.to_owned(),
        task_id: task_id.to_owned(),
        origin_sha256: origin_sha256.to_owned(),
    };
    let path = origin_path(&record)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "runtime target origin must be a regular file"
            );
            anyhow::ensure!(
                metadata.len() <= super::MAX_BYTES,
                "runtime target origin exceeds the read limit"
            );
            super::files::check_private_file(&path)?;
            let stored = read_origin(&path)?;
            Ok(stored == record)
        }
    }
}

fn read_origin(path: &Path) -> anyhow::Result<TargetOrigin> {
    let record: TargetOrigin = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        record.schema_version == 1,
        "unsupported runtime target origin version"
    );
    Ok(record)
}

fn origin_path(origin: &TargetOrigin) -> anyhow::Result<PathBuf> {
    let workspace = uuid::Uuid::parse_str(&origin.workspace_id)?;
    let task = uuid::Uuid::parse_str(&origin.task_id)?;
    let root = super::operator_approval_root()?;
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let projects = root.join("projects");
    super::files::ensure_private_dir(&projects)?;
    let workspace_dir = projects.join(workspace.hyphenated().to_string());
    super::files::ensure_private_dir(&workspace_dir)?;
    let origins = workspace_dir.join("runtime-target-origins");
    super::files::ensure_private_dir(&origins)?;
    anyhow::ensure!(
        origins.canonicalize()?.starts_with(&root),
        "runtime target origin path escapes user state"
    );
    let kind = match &origin.kind {
        TargetOriginKind::CustomApproval => "custom-approval",
        TargetOriginKind::DiscoveredTarget => "discovered-target",
        TargetOriginKind::TrustedAdapterRecipe => "trusted-adapter-recipe",
    };
    Ok(origins.join(format!(
        "{}-{kind}-{}.json",
        task.hyphenated(),
        origin.origin_sha256
    )))
}
