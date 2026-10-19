use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::PlanSnapshotV1;
use crate::core::lifecycle::{GateDecision, GateReceipt};

mod review_validation;
mod snapshot;
pub(super) use snapshot::{is_persisted, load_persisted, record_change};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanReviewRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    receipt: GateReceipt,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prior_receipts: Vec<GateReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verdict: Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
}

pub(super) fn review_change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    if !is_persisted(store, snapshot)? {
        return Err(StoreError::MalformedState(
            "plan review cannot be recorded before its immutable snapshot is persisted".into(),
        ));
    }
    if !super::has_current_user_confirmation(store, snapshot)? {
        return Err(StoreError::MalformedState(
            "plan review cannot be recorded before current user confirmation".into(),
        ));
    }
    review_validation::validate_plan_review(snapshot, receipt)
        .map_err(StoreError::MalformedState)?;
    review_validation::validate_verdict(snapshot, receipt, verdict)
        .map_err(StoreError::MalformedState)?;
    if verdict.pass != (receipt.decision == GateDecision::Approved) {
        return Err(StoreError::MalformedState(
            "plan review decision does not match its structured verdict".into(),
        ));
    }
    let path = review_path(snapshot);
    match read_review(store, &path, snapshot)? {
        Some(existing)
            if existing.receipt == *receipt && existing.verdict.as_ref() == Some(verdict) =>
        {
            Ok(None)
        }
        Some(mut existing) if existing.verdict.is_none() => {
            let now = now_ms();
            existing.prior_receipts.push(existing.receipt);
            existing.receipt = receipt.clone();
            existing.verdict = Some(verdict.clone());
            existing.revision += 1;
            existing.updated_at_ms = now;
            let bytes = serde_json::to_vec_pretty(&existing)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?;
            Ok(Some((
                path.clone(),
                bytes,
                RecordRevisionCheck {
                    path,
                    expected_revision: existing.revision - 1,
                },
            )))
        }
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
                prior_receipts: Vec::new(),
                verdict: Some(verdict.clone()),
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
    Ok(record.receipt.decision == GateDecision::Approved
        && record.verdict.as_ref().is_some_and(|verdict| verdict.pass))
}

pub(super) fn recorded_review_decision(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateDecision>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    Ok(read_review(store, &path, snapshot)?
        .filter(|record| record.verdict.is_some())
        .map(|record| record.receipt.decision))
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

pub(super) fn load_review_verdict(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<crate::harness::prompt_catalog::ReviewVerdictV1>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let Some(record) = read_review(store, &review_path(snapshot), snapshot)? else {
        return Ok(None);
    };
    Ok(record.verdict)
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
            review_validation::validate_plan_review(snapshot, &record.receipt)
                .map_err(StoreError::MalformedState)?;
            for prior_receipt in &record.prior_receipts {
                review_validation::validate_plan_review(snapshot, prior_receipt)
                    .map_err(StoreError::MalformedState)?;
            }
            if let Some(verdict) = &record.verdict {
                review_validation::validate_verdict(snapshot, &record.receipt, verdict)
                    .map_err(StoreError::MalformedState)?;
                if verdict.pass != (record.receipt.decision == GateDecision::Approved) {
                    return Err(StoreError::MalformedState(
                        "plan review decision does not match its structured verdict".into(),
                    ));
                }
            }
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn review_path(snapshot: &PlanSnapshotV1) -> String {
    format!("state/lifecycle_plan_reviews/{}.json", snapshot.uid)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
