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

    pub(crate) fn apply_event(
        &self,
        run_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        event: SelfImprovementEventV1,
    ) -> Result<super::SelfImprovementRunV1, StoreError> {
        self.apply_event_inner(run_uid, expected_revision, event_uid, event, false)
    }

    pub(crate) fn record_active_build_probe(
        &self,
        run_uid: &str,
        expected_revision: u64,
    ) -> Result<super::SelfImprovementRunV1, StoreError> {
        let run = self.load(run_uid)?;
        let Some((active_version, active_commit)) = crate::build_identity::current() else {
            return Ok(run);
        };
        let destination = run
            .destination_ref
            .as_deref()
            .map(|value| value.strip_prefix("refs/heads/").unwrap_or(value));
        let mut merge_records = Vec::with_capacity(run.parent_task_progress.len());
        let parent_evidence_complete = !run.parent_task_progress.is_empty()
            && run.parent_task_progress.iter().all(|parent| {
                let merge_at = parent
                    .pr_merged_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
                let merge_sha = parent.pr_merged_commit_sha.as_deref();
                if let (Some(merge_at), Some(merge_sha)) = (merge_at, merge_sha) {
                    merge_records.push((merge_at, merge_sha));
                }
                parent.code_review_uid.is_some()
                    && parent.runtime_qa_uid.is_some()
                    && parent.spec_receipt_uid.is_some()
                    && parent.pr_head_sha.is_some()
                    && parent.pr_status
                        == Some(crate::core::lifecycle::PullRequestPreparationStatus::Merged)
                    && destination.is_some()
                    && parent.pr_base_branch.as_deref() == destination
                    && merge_sha.is_some_and(crate::core::self_improvement::validation::is_git_sha)
                    && merge_at.is_some()
            });
        if !parent_evidence_complete {
            return Ok(run);
        }
        merge_records.sort_by_key(|(merged_at, _)| *merged_at);
        let Some((latest_time, latest_sha)) = merge_records.last() else {
            return Ok(run);
        };
        if merge_records.len() != run.parent_task_progress.len()
            || merge_records
                .iter()
                .rev()
                .skip(1)
                .any(|(time, sha)| *time == *latest_time && *sha != *latest_sha)
        {
            return Ok(run);
        }
        let merged_sha = run.merged_sha.as_deref().ok_or_else(|| {
            StoreError::MalformedState("self-improvement run has no merged commit".into())
        })?;
        if merged_sha != *latest_sha {
            return Ok(run);
        }
        if merged_sha != active_commit {
            return Ok(run);
        }
        if run.phase == SelfImprovementPhaseV1::MergedNotActivated {
            return Ok(run);
        }
        if run.phase != SelfImprovementPhaseV1::InstalledNotActivated
            || run.installed_version.as_deref() != Some(active_version)
            || run.installed_build_commit_sha.as_deref() != Some(active_commit)
        {
            return Ok(run);
        }
        let evidence = super::super::SelfImprovementActivationEvidenceV1::probe_active_build(&run)
            .map_err(StoreError::MalformedState)?;
        let event_uid = evidence.readiness_probe_uid.clone();
        self.apply_event_inner(
            run_uid,
            expected_revision,
            &event_uid,
            SelfImprovementEventV1::Activated { evidence },
            true,
        )
    }

    pub(crate) fn record_installation_confirmation(
        &self,
        run_uid: &str,
        expected_revision: u64,
        operator_id: &str,
    ) -> Result<super::SelfImprovementRunV1, StoreError> {
        let run = self.load(run_uid)?;
        if run.schema_version < 2
            || run.phase != SelfImprovementPhaseV1::MergedNotActivated
            || !crate::core::self_improvement::validation::valid_text(operator_id, 128)
        {
            return Err(StoreError::MalformedState(
                "installation confirmation requires an identified operator and a merged run".into(),
            ));
        }
        let Some((version, commit)) = crate::build_identity::current() else {
            return Err(StoreError::MalformedState(
                "active build identity is unavailable".into(),
            ));
        };
        if run.merged_sha.as_deref() != Some(commit) {
            return Err(StoreError::MalformedState(
                "running build does not match the merged self-improvement commit".into(),
            ));
        }
        let namespace = uuid::Uuid::parse_str(run_uid)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        let event_uid = uuid::Uuid::new_v5(
            &namespace,
            format!("installed:{operator_id}:{version}:{commit}").as_bytes(),
        )
        .hyphenated()
        .to_string();
        self.apply_event_inner(
            run_uid,
            expected_revision,
            &event_uid,
            SelfImprovementEventV1::Installed {
                version: version.to_owned(),
                build_commit_sha: commit.to_owned(),
                operator_id: operator_id.to_owned(),
            },
            true,
        )
    }

    fn apply_event_inner(
        &self,
        run_uid: &str,
        expected_revision: u64,
        event_uid: &str,
        event: SelfImprovementEventV1,
        active_build_probe: bool,
    ) -> Result<super::SelfImprovementRunV1, StoreError> {
        if matches!(
            event,
            SelfImprovementEventV1::Installed { .. } | SelfImprovementEventV1::Activated { .. }
        ) && !active_build_probe
        {
            return Err(StoreError::MalformedState(
                "installation evidence requires the app-owned active-build probe".into(),
            ));
        }
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
