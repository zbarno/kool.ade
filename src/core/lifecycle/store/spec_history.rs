use serde::{Deserialize, Serialize};

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{SpecDeltaRecordV1, SpecRevisionV1},
};

use super::LifecycleStore;
#[cfg(test)]
use crate::core::lifecycle::SpecRevisionDraftV1;

mod persist;
mod trace;
mod validation;
use persist::current_path;
use validation::validate_history;

#[cfg(test)]
#[path = "spec_history/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpecRevisionHistoryV1 {
    schema_version: u16,
    uid: String,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    feature_uid: String,
    sequence: u64,
    entry: SpecRevisionV1,
    deltas: Vec<SpecDeltaRecordV1>,
}

impl<'a> LifecycleStore<'a> {
    pub fn load_spec_revision(
        &self,
        feature_uid: &str,
    ) -> Result<Option<SpecRevisionV1>, StoreError> {
        let path = current_path(feature_uid)?;
        match self.store.read_record::<SpecRevisionV1>(&path) {
            Ok((record, revision)) => {
                record.validate().map_err(StoreError::MalformedState)?;
                if record.feature_uid != feature_uid || record.revision != revision {
                    return Err(StoreError::MalformedState(
                        "feature specification revision identity is inconsistent".into(),
                    ));
                }
                Ok(Some(record))
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn spec_revision_history(
        &self,
        feature_uid: &str,
    ) -> Result<Vec<(SpecRevisionV1, Vec<SpecDeltaRecordV1>)>, StoreError> {
        validation::require_uuid(feature_uid, "feature")?;
        let mut entries = self
            .store
            .list_record_paths("state/spec_revision_history")?
            .into_iter()
            .map(|path| {
                let (history, revision) = self.store.read_record::<SpecRevisionHistoryV1>(&path)?;
                validate_history(&history, revision)?;
                Ok(history)
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        entries.retain(|entry| entry.feature_uid == feature_uid);
        entries.sort_by_key(|entry| entry.sequence);
        Ok(entries
            .into_iter()
            .map(|entry| (entry.entry, entry.deltas))
            .collect())
    }
}
