use crate::artifacts::planning_store::{RecordRevisionCheck, StoreError};

use super::records::{
    encode, now_unix_ms, read_transition, run_path, transition_path, validate_run_record,
};
use super::{
    RunRecord, SelfImprovementEventV1, SelfImprovementPhaseV1, SelfImprovementStore,
    SelfImprovementTransitionV1, active_runs,
};

impl SelfImprovementStore<'_> {
    pub fn transitions(&self, uid: &str) -> Result<Vec<SelfImprovementTransitionV1>, StoreError> {
        run_path(uid)?;
        let prefix = format!("state/self_improvement_events/{uid}/");
        let mut events = self
            .store
            .list_record_paths(&prefix)?
            .into_iter()
            .map(|path| {
                let record = read_transition(self.store, &path)?.ok_or_else(|| {
                    StoreError::MalformedState(format!(
                        "self-improvement event disappeared: {path}"
                    ))
                })?;
                if record.run_uid != uid {
                    return Err(StoreError::MalformedState(format!(
                        "self-improvement event is inconsistent: {path}"
                    )));
                }
                Ok(record)
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        events.sort_by(|left, right| {
            (left.created_at_ms, &left.uid).cmp(&(right.created_at_ms, &right.uid))
        });
        Ok(events)
    }

    pub fn apply_event(
        &self,
        run_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        event: SelfImprovementEventV1,
    ) -> Result<super::SelfImprovementRunV1, StoreError> {
        let path = run_path(run_uid)?;
        let event_path = transition_path(run_uid, event_uid)?;
        if let Some(existing) = read_transition(self.store, &event_path)? {
            if existing.run_uid != run_uid || existing.event.as_ref() != Some(&event) {
                return Err(StoreError::MalformedState(
                    "self-improvement event identity is already used for different evidence".into(),
                ));
            }
            let current = self.load(run_uid)?;
            return Ok(current);
        }
        let (mut record, actual_revision) = self.store.read_record::<RunRecord>(&path)?;
        validate_run_record(&record, actual_revision, run_uid, &path)?;
        if actual_revision != expected_revision {
            return Err(StoreError::StaleRecordRevision {
                path,
                expected: expected_revision,
                actual: actual_revision,
            });
        }
        let from_phase = record.data.phase;
        record
            .data
            .apply(expected_revision, event.clone())
            .map_err(StoreError::MalformedState)?;
        record.revision = record.data.revision;
        record.updated_at_ms = now_unix_ms();
        let transition = SelfImprovementTransitionV1 {
            schema_version: 1,
            uid: event_uid.to_owned(),
            run_uid: run_uid.to_owned(),
            created_at_ms: now_unix_ms(),
            updated_at_ms: now_unix_ms(),
            revision: 1,
            from_phase,
            to_phase: record.data.phase,
            event: Some(event),
            opportunity_decision_uid: None,
        };
        let mut changes = vec![
            (path.clone(), encode(&record)?),
            (event_path.clone(), encode(&transition)?),
        ];
        let mut checks = vec![
            RecordRevisionCheck {
                path,
                expected_revision,
            },
            RecordRevisionCheck {
                path: event_path,
                expected_revision: 0,
            },
        ];
        if matches!(
            record.data.phase,
            SelfImprovementPhaseV1::Activated | SelfImprovementPhaseV1::Cancelled
        ) && let Some(slot) = active_runs::release(self.store, run_uid)?
        {
            changes.push((slot.path, slot.bytes));
            checks.push(slot.check);
        }
        self.store
            .transaction_with_record_revisions(&changes, &checks)?;
        Ok(record.data)
    }
}
