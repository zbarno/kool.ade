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
        || !valid_evidence_refs(receipt, snapshot)
    {
        return Err(StoreError::MalformedState(
            "user confirmation must approve this exact immutable plan snapshot".into(),
        ));
    }
    let product_hash = crate::core::lifecycle::product_spec::current_product_hash(store)?;
    if confirmed_product_spec_hash(receipt) != Some(product_hash.as_str()) {
        return Err(StoreError::MalformedState(
            "confirmation requires the current product specification".into(),
        ));
    }
    let uid = confirmation_uid(snapshot, &product_hash)?;
    let path = confirmation_path(&uid);
    match store.read_record::<ConfirmationRecord>(&path) {
        Ok((record, actual_revision)) => {
            validate_record(&record, snapshot, actual_revision)?;
            Ok(None)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            let now = now_ms();
            let record = ConfirmationRecord {
                schema_version: 1,
                uid,
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
    Ok(load_receipt(store, snapshot)?.is_some())
}

pub(super) fn is_current(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    let current = crate::core::lifecycle::product_spec::current_product_hash(store)?;
    Ok(load_receipt(store, snapshot)?
        .is_some_and(|receipt| confirmed_product_spec_hash(&receipt) == Some(current.as_str())))
}

pub(super) fn load_receipt(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateReceipt>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let current_hash = crate::core::lifecycle::product_spec::current_product_hash(store).ok();
    let mut receipts = Vec::new();
    for path in store.list_record_paths("state/lifecycle_plan_confirmations")? {
        let (record, revision) = store.read_record::<ConfirmationRecord>(&path)?;
        if record.receipt.subject_uid != snapshot.uid {
            continue;
        }
        validate_record(&record, snapshot, revision)?;
        if path != confirmation_path(&record.uid) {
            return Err(StoreError::MalformedState(
                "confirmation identity does not match its path".into(),
            ));
        }
        receipts.push(record.receipt);
    }
    // Preserve historical consent for the exact immutable plan. Critical
    // generation gates separately require a current product binding; an
    // explicit reconfirmation creates a new immutable record for that binding.
    receipts.sort_by_key(|receipt| {
        (
            confirmed_product_spec_hash(receipt).is_some()
                && confirmed_product_spec_hash(receipt) == current_hash.as_deref(),
            receipt.recorded_at_unix_ms,
            receipt.receipt_uid.clone(),
        )
    });
    Ok(receipts.pop())
}

fn validate_record(
    record: &ConfirmationRecord,
    snapshot: &PlanSnapshotV1,
    actual_revision: u64,
) -> Result<(), StoreError> {
    let receipt = &record.receipt;
    if record.schema_version != 1
        || (record.uid != snapshot.uid
            && confirmed_product_spec_hash(receipt)
                .map(|hash| confirmation_uid(snapshot, hash))
                .transpose()?
                .as_deref()
                != Some(record.uid.as_str()))
        || record.revision != actual_revision
        || record.revision != 1
        || record.created_at_ms == 0
        || record.updated_at_ms < record.created_at_ms
        || receipt.validate().is_err()
        || receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || !(valid_evidence_refs(receipt, snapshot)
            || receipt.evidence_refs == [format!("plan:{}", snapshot.uid)])
    {
        return Err(StoreError::MalformedState(
            "plan confirmation record is malformed or stale".into(),
        ));
    }
    Ok(())
}

fn valid_evidence_refs(receipt: &GateReceipt, snapshot: &PlanSnapshotV1) -> bool {
    receipt.evidence_refs.len() == 2
        && receipt
            .evidence_refs
            .contains(&format!("plan:{}", snapshot.uid))
        && receipt.evidence_refs.iter().any(|reference| {
            reference
                .strip_prefix("product-spec:")
                .is_some_and(crate::core::lifecycle::types::is_sha256)
        })
}

pub(super) fn confirmed_product_spec_hash(receipt: &GateReceipt) -> Option<&str> {
    receipt.evidence_refs.iter().find_map(|reference| {
        reference
            .strip_prefix("product-spec:")
            .filter(|hash| crate::core::lifecycle::types::is_sha256(hash))
    })
}

fn confirmation_uid(snapshot: &PlanSnapshotV1, product_hash: &str) -> Result<String, StoreError> {
    let namespace = uuid::Uuid::parse_str(&snapshot.uid)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(uuid::Uuid::new_v5(
        &namespace,
        format!("product-confirmation:v1:{product_hash}").as_bytes(),
    )
    .hyphenated()
    .to_string())
}

fn confirmation_path(uid: &str) -> String {
    format!("state/lifecycle_plan_confirmations/{uid}.json")
}

#[cfg(test)]
#[path = "confirmation/tests.rs"]
mod tests;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
