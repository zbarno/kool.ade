use serde::Serialize;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::{RunRecord, SelfImprovementTransitionV1};

pub(super) fn validate_run_record(
    record: &RunRecord,
    revision: u64,
    uid: &str,
    path: &str,
) -> Result<(), StoreError> {
    if record.schema_version != 1
        || record.uid != uid
        || record.revision != revision
        || record.data.run_uid != uid
        || record.data.revision != revision
    {
        return Err(StoreError::MalformedState(format!(
            "self-improvement record is inconsistent: {path}"
        )));
    }
    record.data.validate().map_err(StoreError::MalformedState)
}

pub(super) fn read_transition(
    store: &PlanningStore,
    path: &str,
) -> Result<Option<SelfImprovementTransitionV1>, StoreError> {
    match store.read_record::<SelfImprovementTransitionV1>(path) {
        Ok((record, actual_revision)) => {
            if record.schema_version != 1 || record.revision != actual_revision {
                return Err(StoreError::MalformedState(format!(
                    "self-improvement event is inconsistent: {path}"
                )));
            }
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn run_path(uid: &str) -> Result<String, StoreError> {
    canonical_uuid(uid, "self-improvement run")?;
    Ok(format!("state/self_improvements/{uid}.json"))
}

pub(super) fn transition_path(run_uid: &str, event_uid: &str) -> Result<String, StoreError> {
    run_path(run_uid)?;
    canonical_uuid(event_uid, "self-improvement event")?;
    Ok(format!(
        "state/self_improvement_events/{run_uid}/{event_uid}.json"
    ))
}

pub(super) fn canonical_uuid(uid: &str, label: &str) -> Result<(), StoreError> {
    let parsed = uuid::Uuid::parse_str(uid)
        .map_err(|_| StoreError::MalformedState(format!("invalid {label} identity")))?;
    if parsed.hyphenated().to_string() != uid {
        return Err(StoreError::MalformedState(format!(
            "{label} identity is not canonical"
        )));
    }
    Ok(())
}

pub(super) fn encode<T: Serialize>(record: &T) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec_pretty(record).map_err(|error| {
        StoreError::MalformedState(format!(
            "self-improvement record cannot be encoded: {error}"
        ))
    })
}

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
