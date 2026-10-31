use std::path::{Path, PathBuf};

use crate::error::AppError;

use super::{clone_destination, perform_clone_at_with_pre_publish, reject_occupied_destination};

#[path = "pending_clone/receipt.rs"]
mod receipt;
use receipt::{
    CloneIntent, PendingClone, intent_path, read_intent, read_receipt, receipt_path,
    validate_completed_clone, validate_intent, validate_receipt, validate_receipt_identity,
    write_clone_marker, write_intent, write_receipt,
};

pub(crate) fn perform_clone_for_workspace(
    source: &str,
    repo: &str,
    workspace_id: uuid::Uuid,
) -> Result<PathBuf, AppError> {
    let home = home_directory()?;
    perform_clone_at_for_workspace_inner(source, repo, &home, workspace_id)
}

pub(crate) fn pending_clone_workspace_id(
    source: &str,
    repo: &str,
) -> Result<Option<uuid::Uuid>, AppError> {
    let Some(home) = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    pending_clone_workspace_id_at_impl(source, repo, &home)
}

#[cfg(test)]
pub(crate) fn pending_clone_workspace_id_at(
    source: &str,
    repo: &str,
    home: &Path,
) -> Result<Option<uuid::Uuid>, AppError> {
    pending_clone_workspace_id_at_impl(source, repo, home)
}

fn pending_clone_workspace_id_at_impl(
    source: &str,
    repo: &str,
    home: &Path,
) -> Result<Option<uuid::Uuid>, AppError> {
    let destination = clone_destination(repo, home);
    let receipt = read_receipt(&receipt_path(&destination)?)?;
    let intent = read_intent(&intent_path(&destination)?)?;
    if destination_exists(&destination)? {
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        validate_receipt_identity(&receipt, source, &destination)?;
        if let Some(intent) = intent {
            validate_intent(&intent, source, receipt.workspace_id, &destination)?;
        }
        validate_completed_clone(&destination, &receipt)?;
        return Ok(Some(receipt.workspace_id));
    }
    if let Some(receipt) = receipt {
        validate_receipt_identity(&receipt, source, &destination)?;
        if let Some(intent) = intent {
            validate_intent(&intent, source, receipt.workspace_id, &destination)?;
        }
        return Ok(Some(receipt.workspace_id));
    }
    let Some(intent) = intent else {
        return Ok(None);
    };
    validate_intent(&intent, source, intent.workspace_id, &destination)?;
    Ok(Some(intent.workspace_id))
}

pub(crate) fn clear_pending_clone_receipt(
    checkout: &Path,
    source: &str,
    workspace_id: uuid::Uuid,
) -> anyhow::Result<()> {
    let path = receipt_path(checkout).map_err(anyhow::Error::msg)?;
    if let Some(receipt) = read_receipt(&path).map_err(anyhow::Error::msg)? {
        validate_receipt(&receipt, source, workspace_id, checkout).map_err(anyhow::Error::msg)?;
        validate_completed_clone(checkout, &receipt).map_err(anyhow::Error::msg)?;
        std::fs::remove_file(&path)?;
        let marker = receipt::clone_marker_path(checkout).map_err(anyhow::Error::msg)?;
        std::fs::remove_file(marker)?;
    }
    let intent_file = intent_path(checkout).map_err(anyhow::Error::msg)?;
    if let Some(intent) = read_intent(&intent_file).map_err(anyhow::Error::msg)? {
        validate_intent(&intent, source, workspace_id, checkout).map_err(anyhow::Error::msg)?;
        std::fs::remove_file(intent_file)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn perform_clone_at_for_workspace(
    source: &str,
    repo: &str,
    home: &Path,
    workspace_id: uuid::Uuid,
) -> Result<PathBuf, AppError> {
    perform_clone_at_for_workspace_inner(source, repo, home, workspace_id)
}

fn perform_clone_at_for_workspace_inner(
    source: &str,
    repo: &str,
    home: &Path,
    workspace_id: uuid::Uuid,
) -> Result<PathBuf, AppError> {
    let destination = clone_destination(repo, home);
    let clone_receipt_path = receipt_path(&destination)?;
    let intent_file = intent_path(&destination)?;
    let receipt = read_receipt(&clone_receipt_path)?;
    let intent = read_intent(&intent_file)?;
    if destination_exists(&destination)? {
        if let Some(receipt) = receipt {
            validate_receipt(&receipt, source, workspace_id, &destination)?;
            if let Some(intent) = intent {
                validate_intent(&intent, source, workspace_id, &destination)?;
            }
            validate_completed_clone(&destination, &receipt)?;
            return Ok(destination);
        }
        reject_occupied_destination(&destination)?;
    }
    reject_occupied_destination(&destination)?;
    if let Some(receipt) = &receipt {
        validate_receipt(receipt, source, workspace_id, &destination)?;
    }
    if let Some(intent) = &intent {
        validate_intent(intent, source, workspace_id, &destination)?;
    } else {
        let intent = CloneIntent::new(source, workspace_id);
        write_intent(&intent_file, &intent)?;
    }
    let receipt = receipt.unwrap_or_else(|| PendingClone::new(source, workspace_id));
    perform_clone_at_with_pre_publish(source, repo, home, |staging| {
        write_clone_marker(staging, &receipt)?;
        write_receipt(&clone_receipt_path, &receipt)?;
        Ok(())
    })
}

fn destination_exists(path: &Path) -> Result<bool, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::Io {
            op: "inspect managed clone recovery destination".into(),
            detail: format!("{}: {error}", path.to_string_lossy()),
        }),
    }
}

fn home_directory() -> Result<PathBuf, AppError> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Io {
            op: "locate home directory".into(),
            detail: "Set $HOME to a writable directory before cloning.".into(),
        })
}
