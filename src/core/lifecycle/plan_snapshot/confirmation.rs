use serde::{Deserialize, Serialize};

use crate::{
    artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError},
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
};

use super::PlanSnapshotV1;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfirmationRecord {
    schema_version: u16,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    receipt: GateReceipt,
}

pub(super) fn change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    receipt.validate().map_err(StoreError::MalformedState)?;
    if receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || receipt.evidence_refs != vec![format!("plan:{}", snapshot.uid)]
    {
        return Err(StoreError::MalformedState(
            "user confirmation must approve this exact immutable plan snapshot".into(),
        ));
    }
    let path = confirmation_path(snapshot);
    match store.read_record::<ConfirmationRecord>(&path) {
        Ok((record, actual_revision)) => {
            validate_record(&record, snapshot, actual_revision)?;
            Ok(None)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            let now = now_ms();
            let record = ConfirmationRecord {
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
        Err(error) => Err(error),
    }
}

pub(super) fn is_confirmed(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    match store.read_record::<ConfirmationRecord>(&confirmation_path(snapshot)) {
        Ok((record, actual_revision)) => {
            validate_record(&record, snapshot, actual_revision)?;
            Ok(true)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn load_receipt(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateReceipt>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    match store.read_record::<ConfirmationRecord>(&confirmation_path(snapshot)) {
        Ok((record, actual_revision)) => {
            validate_record(&record, snapshot, actual_revision)?;
            Ok(Some(record.receipt))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn validate_record(
    record: &ConfirmationRecord,
    snapshot: &PlanSnapshotV1,
    actual_revision: u64,
) -> Result<(), StoreError> {
    let receipt = &record.receipt;
    if record.schema_version != 1
        || record.uid != snapshot.uid
        || record.revision != actual_revision
        || record.revision != 1
        || record.created_at_ms == 0
        || record.updated_at_ms < record.created_at_ms
        || receipt.validate().is_err()
        || receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || receipt.evidence_refs != vec![format!("plan:{}", snapshot.uid)]
    {
        return Err(StoreError::MalformedState(
            "plan confirmation record is malformed or stale".into(),
        ));
    }
    Ok(())
}

fn confirmation_path(snapshot: &PlanSnapshotV1) -> String {
    format!("state/lifecycle_plan_confirmations/{}.json", snapshot.uid)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
