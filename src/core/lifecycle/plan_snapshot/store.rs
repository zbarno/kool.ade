use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::PlanSnapshotV1;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanReviewRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    receipt: GateReceipt,
}

pub(super) fn review_change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    if !is_persisted(store, snapshot)? {
        return Err(StoreError::MalformedState(
            "plan review cannot be recorded before its immutable snapshot is persisted".into(),
        ));
    }
    validate_plan_review(snapshot, receipt).map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    match read_review(store, &path, snapshot)? {
        Some(existing) if existing.receipt == *receipt => Ok(None),
        Some(_) => Err(StoreError::MalformedState(
            "a different plan review receipt is already recorded".into(),
        )),
        None => {
            let now = now_ms();
            let record = PlanReviewRecord {
                schema_version: 1,
                uid: snapshot.uid.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                revision: 1,
                receipt: receipt.clone(),
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

pub(super) fn has_approved_review(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    let Some(record) = read_review(store, &path, snapshot)? else {
        return Ok(false);
    };
    Ok(record.receipt.decision == GateDecision::Approved)
}

#[cfg(not(test))]
pub(super) fn recorded_review_decision(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateDecision>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    Ok(read_review(store, &path, snapshot)?.map(|record| record.receipt.decision))
}

pub(super) fn load_review_receipt(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateReceipt>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    let Some(record) = read_review(store, &path, snapshot)? else {
        return Ok(None);
    };
    Ok(Some(record.receipt))
}

fn read_review(
    store: &PlanningStore,
    path: &str,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<PlanReviewRecord>, StoreError> {
    match store.read_record::<PlanReviewRecord>(path) {
        Ok((record, revision)) => {
            if record.schema_version != 1
                || record.uid != snapshot.uid
                || record.revision != revision
                || record.created_at_ms == 0
                || record.updated_at_ms < record.created_at_ms
            {
                return Err(StoreError::MalformedState(format!(
                    "invalid plan review record: {path}"
                )));
            }
            validate_plan_review(snapshot, &record.receipt).map_err(StoreError::MalformedState)?;
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn validate_plan_review(snapshot: &PlanSnapshotV1, receipt: &GateReceipt) -> Result<(), String> {
    receipt.validate()?;
    if receipt.actor != GateActor::IndependentReviewer
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || receipt.prompt_role() != Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
    {
        return Err(
            "plan review must be an independent PlanReviewer receipt for this exact snapshot"
                .into(),
        );
    }
    Ok(())
}

fn review_path(snapshot: &PlanSnapshotV1) -> String {
    format!("state/lifecycle_plan_reviews/{}.json", snapshot.uid)
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
