use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

use super::super::PlanSnapshotV1;
#[path = "review/validation.rs"]
mod validation;
use validation::validate_verdict;
#[path = "review/arbitration.rs"]
mod arbitration;
use arbitration::eligible_evidence_id;
pub(crate) use arbitration::{PlanReviewArbitration, arbitration_change};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PlanReviewRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    receipt: GateReceipt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verdict: Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attempts: Vec<PlanReviewAttempt>,
    #[serde(
        default,
        alias = "arbitration",
        deserialize_with = "deserialize_arbitrations",
        skip_serializing_if = "Vec::is_empty"
    )]
    arbitrations: Vec<PlanReviewArbitration>,
}

fn deserialize_arbitrations<'de, D>(deserializer: D) -> Result<Vec<PlanReviewArbitration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }
    Ok(
        match OneOrMany::<PlanReviewArbitration>::deserialize(deserializer)? {
            OneOrMany::One(arbitration) => vec![arbitration],
            OneOrMany::Many(arbitrations) => arbitrations,
        },
    )
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanReviewAttempt {
    receipt: GateReceipt,
    verdict: Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
}

#[cfg(test)]
pub(crate) fn review_change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    review_change_with_verdict(store, snapshot, receipt, None)
}

pub(crate) fn review_change_with_verdict(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
    verdict: Option<&crate::harness::prompt_catalog::ReviewVerdictV1>,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    if !super::super::is_persisted(store, snapshot)? {
        return Err(StoreError::MalformedState(
            "plan review cannot be recorded before its immutable snapshot is persisted".into(),
        ));
    }
    if !super::super::has_current_user_confirmation(store, snapshot)? {
        return Err(StoreError::MalformedState(
            "plan review cannot be recorded before current user confirmation of this exact snapshot".into(),
        ));
    }
    validate_plan_review(snapshot, receipt)?;
    if let Some(verdict) = verdict {
        validate_verdict(snapshot, receipt, verdict).map_err(StoreError::MalformedState)?;
    }
    let path = review_path(snapshot);
    match read_review(store, &path, snapshot)? {
        Some(existing) if existing.receipt == *receipt && existing.verdict.as_ref() == verdict => {
            Ok(None)
        }
        Some(mut existing)
            if existing.verdict.as_ref().is_some_and(is_review_unable)
                && existing.attempts.len() + 1 < MAX_REVIEW_ATTEMPTS =>
        {
            existing.attempts.push(PlanReviewAttempt {
                receipt: existing.receipt.clone(),
                verdict: existing.verdict.clone(),
            });
            existing.receipt = receipt.clone();
            existing.verdict = verdict.cloned();
            existing.updated_at_ms = super::now_ms();
            let expected_revision = existing.revision;
            existing.revision += 1;
            let bytes = serde_json::to_vec_pretty(&existing)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?;
            Ok(Some((
                path.clone(),
                bytes,
                RecordRevisionCheck {
                    path,
                    expected_revision,
                },
            )))
        }
        Some(existing)
            if existing.verdict.as_ref().is_some_and(is_review_unable) =>
        {
            Err(StoreError::MalformedState(
                "bounded independent plan review attempts are exhausted; the plan remains blocked for Manager or user attention".into(),
            ))
        }
        Some(_) => Err(StoreError::MalformedState(
            "a different plan review result is already recorded".into(),
        )),
        None => {
            let now = super::now_ms();
            let record = PlanReviewRecord {
                schema_version: 1,
                uid: snapshot.uid.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                revision: 1,
                receipt: receipt.clone(),
                verdict: verdict.cloned(),
                attempts: Vec::new(),
                arbitrations: Vec::new(),
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

pub(crate) fn has_approved_review(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let Some(record) = read_review(store, &review_path(snapshot), snapshot)? else {
        return Ok(false);
    };
    Ok(record
        .arbitrations
        .last()
        .map(|arbitration| &arbitration.receipt)
        .unwrap_or(&record.receipt)
        .decision
        == GateDecision::Approved)
}

pub(crate) fn recorded_review_decision(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateDecision>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    Ok(
        read_review(store, &review_path(snapshot), snapshot)?.map(|record| {
            record
                .arbitrations
                .last()
                .map(|arbitration| arbitration.receipt.decision)
                .unwrap_or(record.receipt.decision)
        }),
    )
}

pub(crate) fn load_review_receipt(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<GateReceipt>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    Ok(
        read_review(store, &review_path(snapshot), snapshot)?.map(|record| {
            record
                .arbitrations
                .last()
                .map(|arbitration| arbitration.receipt.clone())
                .unwrap_or(record.receipt)
        }),
    )
}

pub(crate) fn load_latest_arbitration(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<(String, GateReceipt)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    Ok(
        read_review(store, &review_path(snapshot), snapshot)?.and_then(|record| {
            record
                .arbitrations
                .last()
                .map(|arbitration| (arbitration.finding_id.clone(), arbitration.receipt.clone()))
        }),
    )
}

pub(crate) fn review_retry_available(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let Some(record) = read_review(store, &review_path(snapshot), snapshot)? else {
        return Ok(false);
    };
    Ok(record.verdict.as_ref().is_some_and(is_review_unable)
        && record.attempts.len() + 1 < MAX_REVIEW_ATTEMPTS)
}

pub(crate) fn load_review_verdict(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<crate::harness::prompt_catalog::ReviewVerdictV1>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    Ok(
        read_review(store, &review_path(snapshot), snapshot)?.and_then(|record| {
            record
                .arbitrations
                .last()
                .map(|arbitration| arbitration.verdict.clone())
                .or(record.verdict)
        }),
    )
}

#[path = "review/record.rs"]
mod record;
use record::{
    MAX_REVIEW_ATTEMPTS, is_review_unable, read_review, review_path, validate_plan_review,
};
