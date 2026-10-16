use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::lifecycle::{SpecSyncQueueEntryV1, SpecSyncQueueStatusV1, SpecSyncTriggerKindV1},
};

use super::{LifecycleStore, records::now_unix_ms};

mod completion;
mod event;
pub(super) use completion::{complete_for_origins, complete_for_task};
pub(super) use event::{queue_path, queued_for_event};

impl LifecycleStore<'_> {
    pub fn list_spec_sync_queue(
        &self,
        feature_uid: &str,
        include_resolved: bool,
    ) -> Result<Vec<SpecSyncQueueEntryV1>, StoreError> {
        require_uuid(feature_uid, "feature")?;
        let mut entries = self
            .store
            .list_record_paths("state/spec_sync_queue")?
            .into_iter()
            .map(|path| {
                let (entry, revision) = self.store.read_record::<SpecSyncQueueEntryV1>(&path)?;
                entry.validate().map_err(StoreError::MalformedState)?;
                if entry.revision != revision || path != queue_path(&entry.uid)? {
                    return Err(StoreError::MalformedState(
                        "specification queue identity is inconsistent".into(),
                    ));
                }
                Ok(entry)
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        entries.retain(|entry| {
            entry.feature_uid == feature_uid
                && (include_resolved || entry.status == SpecSyncQueueStatusV1::Pending)
        });
        entries.sort_by(|left, right| {
            (left.created_at_ms, &left.uid).cmp(&(right.created_at_ms, &right.uid))
        });
        Ok(entries)
    }

    pub fn enqueue_manual_spec_refresh(
        &self,
        feature_uid: &str,
        idempotency_key: &str,
    ) -> Result<SpecSyncQueueEntryV1, StoreError> {
        self.enqueue_manual_spec_refresh_correlated(feature_uid, idempotency_key, None)
    }

    pub fn enqueue_manual_spec_refresh_correlated(
        &self,
        feature_uid: &str,
        idempotency_key: &str,
        request_message_id: Option<&str>,
    ) -> Result<SpecSyncQueueEntryV1, StoreError> {
        require_uuid(feature_uid, "feature")?;
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(StoreError::MalformedState(
                "manual specification refresh key is invalid".into(),
            ));
        }
        if request_message_id.is_some_and(|id| {
            id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control)
        }) {
            return Err(StoreError::MalformedState(
                "manual refresh request message identity is invalid".into(),
            ));
        }
        let feature = self.load_feature(feature_uid)?;
        let uid = uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(feature_uid)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
            format!("manual-spec-refresh:v1:{idempotency_key}").as_bytes(),
        )
        .hyphenated()
        .to_string();
        let now = now_unix_ms();
        let entry = SpecSyncQueueEntryV1 {
            schema_version: 1,
            uid: uid.clone(),
            feature_uid: feature_uid.to_owned(),
            source_event_uid: uid,
            source_task_uid: None,
            request_message_id: request_message_id.map(str::to_owned),
            source_revision: feature.revision,
            trigger: SpecSyncTriggerKindV1::ManualRefresh,
            status: SpecSyncQueueStatusV1::Pending,
            decline_reason: None,
            attempts: 0,
            revision: 1,
            created_at_ms: now,
            updated_at_ms: now,
        };
        entry.validate().map_err(StoreError::MalformedState)?;
        let path = queue_path(&entry.uid)?;
        let bytes = serde_json::to_vec_pretty(&entry)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        match self.store.transaction_with_record_revisions(
            &[(path.clone(), bytes)],
            &[RecordRevisionCheck {
                path: path.clone(),
                expected_revision: 0,
            }],
        ) {
            Ok(_) => Ok(entry),
            Err(StoreError::StaleRecordRevision { .. }) => {
                let (existing, revision) = self.store.read_record::<SpecSyncQueueEntryV1>(&path)?;
                existing.validate().map_err(StoreError::MalformedState)?;
                if revision == existing.revision
                    && existing.feature_uid == feature_uid
                    && existing.trigger == SpecSyncTriggerKindV1::ManualRefresh
                    && existing.request_message_id.as_deref() == request_message_id
                {
                    Ok(existing)
                } else {
                    Err(StoreError::MalformedState(
                        "manual refresh idempotency key conflicts with another event".into(),
                    ))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn decline_spec_sync_queue_entry(
        &self,
        event_uid: &str,
        expected_revision: u64,
        reason: &str,
    ) -> Result<SpecSyncQueueEntryV1, StoreError> {
        if reason.trim().is_empty() || reason.len() > 1024 {
            return Err(StoreError::MalformedState(
                "decline reason is invalid".into(),
            ));
        }
        self.decline_queue_entry(event_uid, expected_revision, reason)
    }

    fn decline_queue_entry(
        &self,
        event_uid: &str,
        expected_revision: u64,
        decline_reason: &str,
    ) -> Result<SpecSyncQueueEntryV1, StoreError> {
        require_uuid(event_uid, "event")?;
        let path = queue_path(event_uid)?;
        let (mut entry, actual) = self.store.read_record::<SpecSyncQueueEntryV1>(&path)?;
        entry.validate().map_err(StoreError::MalformedState)?;
        if entry.status == SpecSyncQueueStatusV1::Declined
            && entry.decline_reason.as_deref() == Some(decline_reason)
        {
            return Ok(entry);
        }
        if actual != expected_revision || entry.revision != expected_revision {
            return Err(StoreError::StaleRecordRevision {
                path,
                expected: expected_revision,
                actual,
            });
        }
        if entry.status != SpecSyncQueueStatusV1::Pending {
            return Err(StoreError::MalformedState(
                "resolved specification queue entry cannot be changed".into(),
            ));
        }
        entry.revision = entry
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("queue revision is exhausted".into()))?;
        entry.updated_at_ms = now_unix_ms();
        entry.status = SpecSyncQueueStatusV1::Declined;
        entry.decline_reason = Some(decline_reason.to_owned());
        entry.validate().map_err(StoreError::MalformedState)?;
        self.store.save_record(&path, &entry, expected_revision)?;
        Ok(entry)
    }
}

fn require_uuid(value: &str, label: &str) -> Result<(), StoreError> {
    if !crate::core::lifecycle::types::is_uuid(value) {
        return Err(StoreError::MalformedState(format!(
            "specification {label} identity is invalid"
        )));
    }
    Ok(())
}
