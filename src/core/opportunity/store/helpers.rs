use serde::Serialize;
use std::path::Path;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::records::OpportunityDedupRecord;

pub(super) fn dedup_path(fingerprint: &str) -> String {
    format!("state/opportunity_dedup/{fingerprint}.json")
}

pub(super) fn read_dedup(
    store: &PlanningStore,
    path: &str,
) -> Result<Option<OpportunityDedupRecord>, StoreError> {
    match store.read_record::<OpportunityDedupRecord>(path) {
        Ok((record, revision))
            if record.schema_version == 1
                && record.revision == revision
                && record.uid
                    == Path::new(path)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default() =>
        {
            Ok(Some(record))
        }
        Ok(_) => Err(StoreError::MalformedState(format!(
            "opportunity dedup record is malformed: {path}"
        ))),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn encode(value: &(impl Serialize + ?Sized)) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec_pretty(value).map_err(|error| StoreError::MalformedState(error.to_string()))
}
