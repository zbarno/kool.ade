use std::path::{Component, Path};

use super::{PlanningStore, StoreError};

/// Expected revision for one mutable record JSON file. Revision zero means the
/// record is expected not to exist yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRevisionCheck {
    pub path: String,
    pub expected_revision: u64,
}

pub(crate) fn validate_record_path(path: &str) -> Result<(), StoreError> {
    let components = Path::new(path).components().collect::<Vec<_>>();
    let valid_collection = components.get(1).is_some_and(|component| {
        matches!(
            component,
            Component::Normal(name)
                if matches!(name.to_str(), Some("work" | "workflow" | "items" | "tasks"))
        )
    });
    let valid_file = components.get(2).is_some_and(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return false;
        };
        name.strip_suffix(".json")
            .is_some_and(|uid| uuid::Uuid::parse_str(uid).is_ok())
    });
    if components.len() != 3
        || !matches!(components.first(), Some(Component::Normal(name)) if name.to_str() == Some("state"))
        || !valid_collection
        || !valid_file
    {
        return Err(StoreError::InvalidPath(path.to_owned()));
    }
    Ok(())
}

pub(crate) fn record_revision(store: &PlanningStore, path: &str) -> Result<u64, StoreError> {
    validate_record_path(path)?;
    let bytes = match store.read(path) {
        Ok(bytes) => bytes,
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(0);
        }
        Err(error) => return Err(error),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        StoreError::MalformedState(format!("record {path} is not valid JSON: {error}"))
    })?;
    let object = value.as_object().ok_or_else(|| {
        StoreError::MalformedState(format!("record {path} must contain a JSON object"))
    })?;
    let uid = object.get("uid").and_then(serde_json::Value::as_str);
    let path_uid = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if uid != Some(path_uid) {
        return Err(StoreError::MalformedState(format!(
            "record {path} UID does not match its filename"
        )));
    }
    for field in ["schemaVersion", "createdAtMs", "updatedAtMs"] {
        if object
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .is_none()
        {
            return Err(StoreError::MalformedState(format!(
                "record {path} has no valid {field}"
            )));
        }
    }
    object
        .get("revision")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| StoreError::MalformedState(format!("record {path} has no revision")))
}

pub(crate) fn validate_record_collection(path: &str) -> Result<(), StoreError> {
    if matches!(
        path,
        "state/work" | "state/workflow" | "state/items" | "state/tasks"
    ) {
        Ok(())
    } else {
        Err(StoreError::InvalidPath(path.to_owned()))
    }
}

pub(crate) fn validate_record_revisions(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_records: &[RecordRevisionCheck],
) -> Result<(), StoreError> {
    let mut checked = std::collections::BTreeSet::new();
    for check in expected_records {
        validate_record_path(&check.path)?;
        if !checked.insert(&check.path) {
            return Err(StoreError::MalformedState(format!(
                "duplicate record revision check for {}",
                check.path
            )));
        }
        let Some((_, bytes)) = changes.iter().find(|(path, _)| path == &check.path) else {
            return Err(StoreError::MalformedState(format!(
                "record revision check for {} has no corresponding write",
                check.path
            )));
        };
        let actual = record_revision(store, &check.path)?;
        if actual != check.expected_revision {
            return Err(StoreError::StaleRecordRevision {
                path: check.path.clone(),
                expected: check.expected_revision,
                actual,
            });
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
            StoreError::MalformedState(format!(
                "new record {} is not valid JSON: {error}",
                check.path
            ))
        })?;
        let next = value
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                StoreError::MalformedState(format!("new record {} has no revision", check.path))
            })?;
        if next != check.expected_revision.saturating_add(1) {
            return Err(StoreError::MalformedState(format!(
                "new record {} must increment its revision by one",
                check.path
            )));
        }
        let object = value.as_object().ok_or_else(|| {
            StoreError::MalformedState(format!("new record {} must be a JSON object", check.path))
        })?;
        let uid = object.get("uid").and_then(serde_json::Value::as_str);
        if uid
            != Path::new(&check.path)
                .file_stem()
                .and_then(|stem| stem.to_str())
        {
            return Err(StoreError::MalformedState(format!(
                "new record {} UID does not match its filename",
                check.path
            )));
        }
        for field in ["schemaVersion", "createdAtMs", "updatedAtMs"] {
            if object
                .get(field)
                .and_then(serde_json::Value::as_u64)
                .is_none()
            {
                return Err(StoreError::MalformedState(format!(
                    "new record {} has no valid {field}",
                    check.path
                )));
            }
        }
    }
    Ok(())
}
