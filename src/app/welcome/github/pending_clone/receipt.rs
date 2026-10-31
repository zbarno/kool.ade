use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PendingClone {
    pub(super) schema_version: u32,
    pub(super) source: String,
    pub(super) workspace_id: uuid::Uuid,
    pub(super) token: uuid::Uuid,
}

impl PendingClone {
    pub(super) fn new(source: &str, workspace_id: uuid::Uuid) -> Self {
        Self {
            schema_version: 2,
            source: source.to_owned(),
            workspace_id,
            token: uuid::Uuid::new_v4(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CloneIntent {
    pub(super) schema_version: u32,
    pub(super) source: String,
    pub(super) workspace_id: uuid::Uuid,
}

impl CloneIntent {
    pub(super) fn new(source: &str, workspace_id: uuid::Uuid) -> Self {
        Self {
            schema_version: 1,
            source: source.to_owned(),
            workspace_id,
        }
    }
}

pub(super) fn receipt_path(checkout: &Path) -> Result<PathBuf, AppError> {
    sidecar_path(checkout, "koolade-pending-clone.json")
}

pub(super) fn intent_path(checkout: &Path) -> Result<PathBuf, AppError> {
    sidecar_path(checkout, "koolade-pending-setup.json")
}

fn sidecar_path(checkout: &Path, suffix: &str) -> Result<PathBuf, AppError> {
    let parent = checkout.parent().ok_or_else(|| AppError::InvalidRepo {
        path: checkout.to_string_lossy().into_owned(),
        detail: "managed clone destination has no parent folder".into(),
    })?;
    let name = checkout
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .ok_or_else(|| AppError::InvalidRepo {
            path: checkout.to_string_lossy().into_owned(),
            detail: "managed clone destination name is invalid".into(),
        })?;
    Ok(parent.join(format!(".{name}.{suffix}")))
}

pub(super) fn read_receipt(path: &Path) -> Result<Option<PendingClone>, AppError> {
    let Some(bytes) = read_bytes(path, "clone receipt")? else {
        return Ok(None);
    };
    let receipt: PendingClone = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_receipt(path, "clone receipt is unreadable"))?;
    if receipt.schema_version != 2 {
        return Err(invalid_receipt(
            path,
            "pending setup record version is unsupported",
        ));
    }
    Ok(Some(receipt))
}

pub(super) fn read_intent(path: &Path) -> Result<Option<CloneIntent>, AppError> {
    let Some(bytes) = read_bytes(path, "setup intent")? else {
        return Ok(None);
    };
    let intent: CloneIntent = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_receipt(path, "setup intent is unreadable"))?;
    if intent.schema_version != 1 || intent.workspace_id.is_nil() {
        return Err(invalid_receipt(
            path,
            "setup intent version or workspace ID is invalid",
        ));
    }
    Ok(Some(intent))
}

pub(super) fn write_receipt(path: &Path, receipt: &PendingClone) -> Result<(), AppError> {
    write_record(path, receipt, "write managed clone recovery receipt")
}

pub(super) fn write_intent(path: &Path, intent: &CloneIntent) -> Result<(), AppError> {
    write_record(path, intent, "write managed clone setup intent")
}

fn write_record<T: Serialize>(path: &Path, record: &T, operation: &str) -> Result<(), AppError> {
    let parent = path.parent().expect("receipt path has a parent");
    fs::create_dir_all(parent).map_err(|error| receipt_io_error(parent, "create", error))?;
    let bytes =
        serde_json::to_vec(record).map_err(|error| invalid_receipt(path, &error.to_string()))?;
    crate::artifacts::atomic_write_bytes(path, &bytes).map_err(|error| AppError::Io {
        op: operation.into(),
        detail: format!("{}: {error:#}", path.to_string_lossy()),
    })
}

pub(super) fn write_clone_marker(checkout: &Path, receipt: &PendingClone) -> Result<(), AppError> {
    let path = clone_marker_path(checkout)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(invalid_receipt(
                checkout,
                "new clone staging metadata already contains a recovery marker",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(receipt_io_error(&path, "inspect", error)),
    }
    write_receipt(&path, receipt)
}

pub(super) fn clone_marker_path(checkout: &Path) -> Result<PathBuf, AppError> {
    let git_dir = checkout.join(".git");
    let metadata = fs::symlink_metadata(&git_dir).map_err(|error| {
        invalid_receipt(
            checkout,
            &format!("the cloned checkout has no private Git metadata: {error}"),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_receipt(
            checkout,
            "the cloned checkout's Git metadata is not a real directory",
        ));
    }
    Ok(git_dir.join("koolade-pending-clone.json"))
}

pub(super) fn validate_receipt_identity(
    receipt: &PendingClone,
    source: &str,
    checkout: &Path,
) -> Result<(), AppError> {
    if receipt.schema_version != 2
        || receipt.source != source
        || receipt.workspace_id.is_nil()
        || receipt.token.is_nil()
    {
        return Err(invalid_receipt(
            checkout,
            "the saved setup record does not match this repository and workspace",
        ));
    }
    Ok(())
}

pub(super) fn validate_receipt(
    receipt: &PendingClone,
    source: &str,
    workspace_id: uuid::Uuid,
    checkout: &Path,
) -> Result<(), AppError> {
    validate_receipt_identity(receipt, source, checkout)?;
    if receipt.workspace_id != workspace_id {
        return Err(invalid_receipt(
            checkout,
            "the pending clone belongs to a different managed workspace",
        ));
    }
    Ok(())
}

pub(super) fn validate_completed_clone(
    checkout: &Path,
    receipt: &PendingClone,
) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(checkout).map_err(|error| {
        invalid_receipt(
            checkout,
            &format!("the interrupted managed clone is unavailable: {error}"),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_receipt(
            checkout,
            "the interrupted managed clone destination is not a real directory",
        ));
    }
    if !crate::core::gitops::is_work_tree(checkout)
        || crate::core::gitops::origin_url(checkout).as_deref() != Some(receipt.source.as_str())
    {
        return Err(invalid_receipt(
            checkout,
            "the interrupted managed clone no longer matches its recorded GitHub repository",
        ));
    }
    let marker_path = clone_marker_path(checkout)?;
    let marker = read_receipt(&marker_path)?.ok_or_else(|| {
        invalid_receipt(
            checkout,
            "the checkout was not published by this pending clone operation",
        )
    })?;
    if marker != *receipt {
        return Err(invalid_receipt(
            checkout,
            "the checkout's private clone marker does not match the saved setup record",
        ));
    }
    Ok(())
}

pub(super) fn validate_intent(
    intent: &CloneIntent,
    source: &str,
    workspace_id: uuid::Uuid,
    path: &Path,
) -> Result<(), AppError> {
    if intent.schema_version != 1
        || intent.source != source
        || intent.workspace_id != workspace_id
        || intent.workspace_id.is_nil()
    {
        return Err(invalid_receipt(
            path,
            "the saved setup intent does not match this repository and workspace",
        ));
    }
    Ok(())
}

fn read_bytes(path: &Path, label: &str) -> Result<Option<Vec<u8>>, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(receipt_io_error(path, "inspect", error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_receipt(
            path,
            &format!("{label} is not a regular file"),
        ));
    }
    if metadata.len() > 4096 {
        return Err(invalid_receipt(path, &format!("{label} is too large")));
    }
    let bytes = fs::read(path).map_err(|error| receipt_io_error(path, "read", error))?;
    Ok(Some(bytes))
}

fn invalid_receipt(path: &Path, detail: &str) -> AppError {
    AppError::InvalidRepo {
        path: path.to_string_lossy().into_owned(),
        detail: format!("managed clone recovery was refused: {detail}"),
    }
}

fn receipt_io_error(path: &Path, operation: &str, error: std::io::Error) -> AppError {
    AppError::Io {
        op: format!("{operation} managed clone recovery record"),
        detail: format!("{}: {error}", path.to_string_lossy()),
    }
}
