use crate::{
    artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError},
    core::opportunity::OpportunityDiscoverySettingsV1,
};
use serde::{Deserialize, Serialize};

const PATH: &str = "state/opportunity_discovery_settings.json";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsRecord {
    schema_version: u16,
    revision: u64,
    data: OpportunityDiscoverySettingsV1,
}

pub(super) fn load(store: &PlanningStore) -> Result<OpportunityDiscoverySettingsV1, StoreError> {
    match store.read_record::<SettingsRecord>(PATH) {
        Ok((record, actual_revision)) => {
            if record.schema_version != 1
                || record.revision != actual_revision
                || record.data.revision != actual_revision
            {
                return Err(StoreError::MalformedState(
                    "opportunity discovery settings record is inconsistent".into(),
                ));
            }
            record.data.validate().map_err(StoreError::MalformedState)?;
            Ok(record.data)
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(OpportunityDiscoverySettingsV1::default())
        }
        Err(error) => Err(error),
    }
}

pub(super) fn save(
    store: &PlanningStore,
    settings: &OpportunityDiscoverySettingsV1,
    expected_revision: u64,
) -> Result<OpportunityDiscoverySettingsV1, StoreError> {
    settings.validate().map_err(StoreError::MalformedState)?;
    if settings.revision != expected_revision {
        return Err(StoreError::MalformedState(
            "settings payload revision does not match the expected revision".into(),
        ));
    }
    let current = load(store)?;
    if current.revision != expected_revision {
        return Err(StoreError::StaleRecordRevision {
            path: PATH.into(),
            expected: expected_revision,
            actual: current.revision,
        });
    }
    let next_revision = expected_revision
        .checked_add(1)
        .ok_or_else(|| StoreError::MalformedState("settings revision is exhausted".into()))?;
    let mut next = settings.clone();
    next.revision = next_revision;
    let record = SettingsRecord {
        schema_version: 1,
        revision: next_revision,
        data: next.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    store.transaction_with_record_revisions(
        &[(PATH.into(), bytes)],
        &[RecordRevisionCheck {
            path: PATH.into(),
            expected_revision,
        }],
    )?;
    Ok(next)
}
