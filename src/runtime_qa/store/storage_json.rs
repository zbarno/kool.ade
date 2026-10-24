use std::path::Path;

use super::storage_permissions::{ensure_private_directory_chain, secure_private_file};

pub(super) fn read_json<T: serde::de::DeserializeOwned>(
    root: &Path,
    path: &Path,
) -> Result<T, String> {
    if std::fs::symlink_metadata(path).is_ok() {
        secure_private_file(root, path)?;
    }
    let bytes = std::fs::read(path).map_err(|error| {
        format!(
            "cannot read persisted runtime QA record {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "persisted runtime QA record {} is malformed: {error}",
            path.display()
        )
    })
}

pub(super) fn create_or_match_json<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq,
>(
    root: &Path,
    path: &Path,
    value: &T,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "runtime QA record path has no parent directory".to_owned())?;
    ensure_private_directory_chain(root, parent)?;
    match read_json::<T>(root, path) {
        Ok(existing) if &existing == value => secure_private_file(root, path),
        Ok(_) => Err("runtime QA request identity is already bound to different content".into()),
        Err(_error) if !path.exists() => create_json(root, path, value),
        Err(error) => Err(error),
    }
}

pub(super) fn safe_uuid(value: &str) -> Result<&str, String> {
    if uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value) {
        Ok(value)
    } else {
        Err("runtime QA record identifier is invalid".into())
    }
}

pub(super) fn create_json(
    root: &Path,
    path: &Path,
    value: &(impl serde::Serialize + ?Sized),
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "runtime QA record path has no parent directory".to_owned())?;
    ensure_private_directory_chain(root, parent)?;
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "runtime QA record cannot be encoded")?;
    crate::artifacts::atomic_create_bytes(path, &bytes)
        .map_err(|error| format!("cannot create immutable runtime QA record: {error}"))?;
    secure_private_file(root, path)
}
