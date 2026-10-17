use std::collections::HashSet;

use crate::artifacts::planning_store::StoreError;

use super::super::SpecRevisionHistory;

pub(in crate::core::lifecycle::store) fn revision_uid(
    feature_uid: &str,
    sequence: u64,
) -> Result<String, StoreError> {
    let feature = uuid::Uuid::parse_str(feature_uid)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(
        uuid::Uuid::new_v5(&feature, format!("spec-revision:v1:{sequence}").as_bytes())
            .hyphenated()
            .to_string(),
    )
}

pub(in crate::core::lifecycle::store) fn validate_history(
    history: &SpecRevisionHistory,
    record_revision: u64,
) -> Result<(), StoreError> {
    history
        .entry
        .validate()
        .map_err(StoreError::MalformedState)?;
    let expected_uid = revision_uid(&history.feature_uid, history.sequence)?;
    let mut requirements = HashSet::new();
    if !matches!(history.schema_version, 1 | 2)
        || (history.schema_version == 2 && history.origin_dispositions.is_none())
        || (history.schema_version == 1 && history.origin_dispositions.is_some())
        || history.uid != expected_uid
        || record_revision != 1
        || history.revision != record_revision
        || history.feature_uid != history.entry.feature_uid
        || history.sequence != history.entry.seq
        || history.created_at_ms == 0
        || history.updated_at_ms < history.created_at_ms
        || history.deltas.len() > 256
        || history.deltas.iter().any(|delta| {
            delta.validate().is_err()
                || !history.entry.origin_events.contains(&delta.event_id)
                || !requirements.insert(delta.requirement_id.as_str())
        })
    {
        return Err(StoreError::MalformedState(
            "immutable specification revision history is inconsistent".into(),
        ));
    }
    super::super::origins::validate(history)?;
    Ok(())
}
