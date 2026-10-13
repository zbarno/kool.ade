use crate::{
    artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError},
    core::opportunity::{OpportunityOriginV1, OpportunityV1},
};
use serde::{Deserialize, Serialize};

use super::{now_unix_ms, settings};

pub(super) struct Reservation {
    pub path: String,
    pub bytes: Vec<u8>,
    pub check: RecordRevisionCheck,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyQuotaRecord {
    schema_version: u16,
    date_utc: String,
    revision: u64,
    proposal_count: u8,
}

pub(super) fn reserve(
    store: &PlanningStore,
    proposal: &OpportunityV1,
) -> Result<Reservation, StoreError> {
    let preferences = settings::load(store)?;
    if proposal.origin == OpportunityOriginV1::ScheduledExploration
        && preferences.exploration != super::super::OpportunityExploreModeV1::OptedIn
    {
        return Err(StoreError::MalformedState(
            "scheduled opportunity exploration is not enabled for this workspace".into(),
        ));
    }
    let now = now_unix_ms();
    let now = i64::try_from(now)
        .map_err(|_| StoreError::MalformedState("current time is out of range".into()))?;
    let date_utc = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now)
        .ok_or_else(|| StoreError::MalformedState("current time is out of range".into()))?
        .format("%Y-%m-%d")
        .to_string();
    let path = format!("state/opportunity_quota/{date_utc}.json");
    let current = match store.read_record::<DailyQuotaRecord>(&path) {
        Ok((record, actual_revision)) => {
            if record.schema_version != 1
                || record.date_utc != date_utc
                || record.revision != actual_revision
                || record.proposal_count as u64 != actual_revision
            {
                return Err(StoreError::MalformedState(
                    "daily opportunity quota record is inconsistent".into(),
                ));
            }
            record
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            DailyQuotaRecord {
                schema_version: 1,
                date_utc: date_utc.clone(),
                revision: 0,
                proposal_count: 0,
            }
        }
        Err(error) => return Err(error),
    };
    if current.proposal_count >= preferences.daily_proposal_limit {
        return Err(StoreError::MalformedState(
            "daily opportunity proposal limit has been reached".into(),
        ));
    }
    let next = DailyQuotaRecord {
        schema_version: 1,
        date_utc,
        revision: current.revision + 1,
        proposal_count: current.proposal_count + 1,
    };
    let bytes = serde_json::to_vec_pretty(&next)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(Reservation {
        path: path.clone(),
        bytes,
        check: RecordRevisionCheck {
            path,
            expected_revision: current.revision,
        },
    })
}
