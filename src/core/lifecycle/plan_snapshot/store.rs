use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::PlanSnapshotV1;
#[path = "store/review.rs"]
mod review;
pub(super) use review::load_review_verdict;
pub(super) use review::recorded_review_decision;
#[cfg(test)]
pub(super) use review::review_change;
pub(super) use review::review_retry_available;
pub(super) use review::{
    arbitration_change, has_approved_review, load_review_receipt, review_change_with_verdict,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanSnapshotRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    data: PlanSnapshotV1,
}

/// Returns an immutable create-only record change; identical snapshots are idempotent.
pub(super) fn record_change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = snapshot_path(snapshot);
    match read_record(store, &path, snapshot)? {
        Some(_) => Ok(None),
        None => {
            let now = now_ms();
            let record = PlanSnapshotRecord {
                schema_version: 1,
                uid: snapshot.uid.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                revision: 1,
                data: snapshot.clone(),
            };
            let bytes = serde_json::to_vec_pretty(&record)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?;
            Ok(Some((
                path.clone(),
                bytes,
                RecordRevisionCheck {
                    path,
                    expected_revision: 0,
                },
            )))
        }
    }
}

pub(super) fn is_persisted(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    Ok(read_record(store, &snapshot_path(snapshot), snapshot)?.is_some())
}

pub(super) fn load_persisted(
    store: &PlanningStore,
    uid: &str,
) -> Result<Option<PlanSnapshotV1>, StoreError> {
    if !crate::core::lifecycle::types::is_uuid(uid) {
        return Err(StoreError::MalformedState(
            "plan snapshot identity is invalid".into(),
        ));
    }
    let path = format!("state/lifecycle_plans/{uid}.json");
    match store.read_record::<PlanSnapshotRecord>(&path) {
        Ok((record, revision)) => {
            if record.schema_version != 1
                || record.uid != uid
                || record.revision != revision
                || record.data.uid != uid
                || record.created_at_ms == 0
                || record.updated_at_ms < record.created_at_ms
            {
                return Err(StoreError::MalformedState(
                    "persisted plan snapshot identity is inconsistent".into(),
                ));
            }
            record.data.validate().map_err(StoreError::MalformedState)?;
            Ok(Some(record.data))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_record(
    store: &PlanningStore,
    path: &str,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<PlanSnapshotRecord>, StoreError> {
    match store.read_record::<PlanSnapshotRecord>(path) {
        Ok((record, revision)) => {
            if record.schema_version != 1
                || record.uid != snapshot.uid
                || record.revision != revision
                || record.data != *snapshot
                || record.created_at_ms == 0
                || record.updated_at_ms < record.created_at_ms
            {
                return Err(StoreError::MalformedState(format!(
                    "immutable plan snapshot identity or contents differ: {path}"
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

fn snapshot_path(snapshot: &PlanSnapshotV1) -> String {
    format!("state/lifecycle_plans/{}.json", snapshot.uid)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
