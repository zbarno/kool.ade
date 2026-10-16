use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::lifecycle::{SpecSyncQueueEntryV1, SpecSyncQueueStatusV1},
};

use super::super::records::now_unix_ms;
use super::{queue_path, require_uuid};

pub(in crate::core::lifecycle::store) fn complete_for_task(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    task_uid: &str,
) -> Result<super::super::records::AdditionalTransition, StoreError> {
    require_uuid(feature_uid, "feature")?;
    require_uuid(task_uid, "task")?;
    let mut additional = super::super::records::AdditionalTransition {
        changes: Vec::new(),
        checks: Vec::new(),
    };
    for path in store.list_record_paths("state/spec_sync_queue")? {
        let (mut entry, actual) = store.read_record::<SpecSyncQueueEntryV1>(&path)?;
        entry.validate().map_err(StoreError::MalformedState)?;
        if entry.feature_uid != feature_uid
            || entry.source_task_uid.as_deref() != Some(task_uid)
            || entry.status != SpecSyncQueueStatusV1::Pending
        {
            continue;
        }
        if actual != entry.revision {
            return Err(StoreError::MalformedState(
                "task reconciliation queue record revision is inconsistent".into(),
            ));
        }
        entry.revision = entry
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("queue revision is exhausted".into()))?;
        entry.updated_at_ms = now_unix_ms();
        entry.status = SpecSyncQueueStatusV1::Completed;
        entry.attempts = entry.attempts.saturating_add(1);
        entry.validate().map_err(StoreError::MalformedState)?;
        additional.changes.push((
            path.clone(),
            serde_json::to_vec_pretty(&entry)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
        ));
        additional.checks.push(RecordRevisionCheck {
            path,
            expected_revision: actual,
        });
    }
    Ok(additional)
}

pub(in crate::core::lifecycle::store) fn complete_for_origins(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    origin_event_uids: &[String],
) -> Result<super::super::records::AdditionalTransition, StoreError> {
    require_uuid(feature_uid, "feature")?;
    let mut additional = super::super::records::AdditionalTransition {
        changes: Vec::new(),
        checks: Vec::new(),
    };
    for event_uid in origin_event_uids {
        require_uuid(event_uid, "origin event")?;
        let path = queue_path(event_uid)?;
        let (mut entry, actual) = store.read_record::<SpecSyncQueueEntryV1>(&path)?;
        entry.validate().map_err(StoreError::MalformedState)?;
        if entry.feature_uid != feature_uid {
            return Err(StoreError::MalformedState(
                "origin queue entry belongs to another feature".into(),
            ));
        }
        if entry.status != SpecSyncQueueStatusV1::Pending {
            continue;
        }
        if actual != entry.revision {
            return Err(StoreError::MalformedState(
                "origin queue entry revision is inconsistent".into(),
            ));
        }
        entry.revision = entry
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("queue revision is exhausted".into()))?;
        entry.updated_at_ms = now_unix_ms();
        entry.status = SpecSyncQueueStatusV1::Completed;
        entry.attempts = entry.attempts.saturating_add(1);
        entry.validate().map_err(StoreError::MalformedState)?;
        additional.changes.push((
            path.clone(),
            serde_json::to_vec_pretty(&entry)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
        ));
        additional.checks.push(RecordRevisionCheck {
            path,
            expected_revision: actual,
        });
    }
    Ok(additional)
}
