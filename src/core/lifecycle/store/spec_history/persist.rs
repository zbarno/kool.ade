use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::lifecycle::{SpecReconciliationStatusV1, SpecRevisionDraftV1, SpecRevisionV1},
};

use super::super::{records::now_unix_ms, spec_sync_queue};
use super::origins::draft_matches;
use super::{
    LifecycleStore, SpecRevisionHistory,
    validation::{self, revision_uid, validate_history},
};

impl LifecycleStore<'_> {
    /// Append one immutable reconciliation revision and update the current
    /// per-feature projection in the same optimistic-concurrency transaction.
    pub fn append_spec_revision(
        &self,
        feature_uid: &str,
        expected_sequence: u64,
        draft: SpecRevisionDraftV1,
    ) -> Result<SpecRevisionV1, StoreError> {
        self.append_spec_revision_with_changes(
            feature_uid,
            expected_sequence,
            draft,
            None,
            Vec::new(),
        )
    }

    /// Append a revision and commit related canonical-document changes in the
    /// same optimistic-concurrency transaction.
    pub fn append_spec_revision_with_changes(
        &self,
        feature_uid: &str,
        expected_sequence: u64,
        draft: SpecRevisionDraftV1,
        expected_store_revision: Option<&str>,
        related_changes: Vec<(String, Vec<u8>)>,
    ) -> Result<SpecRevisionV1, StoreError> {
        self.append_spec_revision_inner(
            feature_uid,
            expected_sequence,
            draft,
            expected_store_revision,
            None,
            related_changes,
        )
    }

    pub(crate) fn append_spec_revision_with_feature_authorization(
        &self,
        feature_uid: &str,
        expected_sequence: u64,
        draft: SpecRevisionDraftV1,
        expected_store_revision: Option<&str>,
        authorization: crate::core::lifecycle::FeatureDeltaAuthorizationV1,
        related_changes: Vec<(String, Vec<u8>)>,
    ) -> Result<SpecRevisionV1, StoreError> {
        self.append_spec_revision_inner(
            feature_uid,
            expected_sequence,
            draft,
            expected_store_revision,
            Some(authorization),
            related_changes,
        )
    }

    fn append_spec_revision_inner(
        &self,
        feature_uid: &str,
        expected_sequence: u64,
        draft: SpecRevisionDraftV1,
        expected_store_revision: Option<&str>,
        authorization: Option<crate::core::lifecycle::FeatureDeltaAuthorizationV1>,
        related_changes: Vec<(String, Vec<u8>)>,
    ) -> Result<SpecRevisionV1, StoreError> {
        validation::require_uuid(feature_uid, "feature")?;
        let current_path = current_path(feature_uid)?;
        let (current, actual_record_revision) = match self
            .store
            .read_record::<SpecRevisionV1>(&current_path)
        {
            Ok(record) => (Some(record.0), record.1),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                (None, 0)
            }
            Err(error) => return Err(error),
        };
        let actual_sequence = current.as_ref().map_or(0, |record| record.seq);
        if let Some(current) = current.as_ref() {
            let history_path = format!(
                "state/spec_revision_history/{}.json",
                revision_uid(feature_uid, actual_sequence)?
            );
            let (history, revision) = self
                .store
                .read_record::<SpecRevisionHistory>(&history_path)?;
            validate_history(&history, revision)?;
            if draft_matches(&draft, &history) {
                return Ok(current.clone());
            }
        }
        if actual_sequence == expected_sequence.saturating_add(1) {
            let history_path = format!(
                "state/spec_revision_history/{}.json",
                revision_uid(feature_uid, actual_sequence)?
            );
            let (history, revision) = self
                .store
                .read_record::<SpecRevisionHistory>(&history_path)?;
            validate_history(&history, revision)?;
            if history.entry.feature_uid == feature_uid && draft_matches(&draft, &history) {
                return Ok(history.entry);
            }
        }
        if actual_sequence != expected_sequence || actual_record_revision != expected_sequence {
            return Err(StoreError::StaleRecordRevision {
                path: current_path,
                expected: expected_sequence,
                actual: actual_record_revision,
            });
        }
        if let Some(authorization) = authorization.as_ref() {
            authorization
                .validate()
                .map_err(StoreError::MalformedState)?;
            if authorization.feature_uid != feature_uid
                || authorization.source_spec_sequence != expected_sequence
                || expected_store_revision.is_none()
            {
                return Err(StoreError::MalformedState(
                    "feature authorization is not bound to this revision transaction".into(),
                ));
            }
            let path = format!(
                "state/lifecycle_feature_delta_authorizations/{}.json",
                authorization.receipt_uid
            );
            match self.store.read(&path) {
                Ok(_) => {
                    return Err(StoreError::MalformedState(
                        "feature authorization receipts are immutable".into(),
                    ));
                }
                Err(StoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let projected_content_hash = if related_changes.is_empty() {
            None
        } else {
            expected_store_revision.ok_or_else(|| {
                StoreError::MalformedState(
                    "projected product content requires a store revision fence".into(),
                )
            })?;
            Some(draft.content_hash.as_str())
        };
        validation::validate_draft(
            self,
            feature_uid,
            expected_sequence,
            &draft,
            projected_content_hash,
            authorization.as_ref(),
        )?;
        let next_sequence = expected_sequence
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("spec revision is exhausted".into()))?;
        let now = now_unix_ms();
        let origin_dispositions = Some(super::origins::for_draft(&draft)?);
        let entry = SpecRevisionV1 {
            schema_version: 1,
            uid: feature_uid.to_owned(),
            feature_uid: feature_uid.to_owned(),
            revision: next_sequence,
            seq: next_sequence,
            content_hash: draft.content_hash,
            source_revision: draft.source_revision,
            produced_by_run_id: draft.produced_by_run_id,
            origin_events: draft.origin_events,
            decision_refs: draft.decision_refs,
            approved_target_hash: draft.approved_target_hash,
            implementation_head_sha: draft.implementation_head_sha,
            reconciliation_status: draft.reconciliation_status,
            outcome_reason: draft.outcome_reason,
            created_at_ms: current.as_ref().map_or(now, |record| record.created_at_ms),
            updated_at_ms: now,
        };
        entry.validate().map_err(StoreError::MalformedState)?;
        let history_uid = revision_uid(feature_uid, next_sequence)?;
        let history = SpecRevisionHistory {
            schema_version: 2,
            uid: history_uid.clone(),
            revision: 1,
            created_at_ms: now,
            updated_at_ms: now,
            feature_uid: feature_uid.to_owned(),
            sequence: next_sequence,
            entry: entry.clone(),
            deltas: draft.deltas,
            origin_dispositions,
        };
        validate_history(&history, 1)?;
        let history_path = format!("state/spec_revision_history/{history_uid}.json");
        let mut changes = vec![
            (
                current_path.clone(),
                serde_json::to_vec_pretty(&entry)
                    .map_err(|error| StoreError::MalformedState(error.to_string()))?,
            ),
            (
                history_path.clone(),
                serde_json::to_vec_pretty(&history)
                    .map_err(|error| StoreError::MalformedState(error.to_string()))?,
            ),
        ];
        let mut checks = vec![
            RecordRevisionCheck {
                path: current_path,
                expected_revision: expected_sequence,
            },
            RecordRevisionCheck {
                path: history_path,
                expected_revision: 0,
            },
        ];
        for (path, bytes) in related_changes {
            changes.push((path, bytes));
        }
        if let Some(authorization) = authorization {
            let path = format!(
                "state/lifecycle_feature_delta_authorizations/{}.json",
                authorization.receipt_uid
            );
            changes.push((
                path,
                serde_json::to_vec_pretty(&authorization)
                    .map_err(|error| StoreError::MalformedState(error.to_string()))?,
            ));
        }
        if matches!(
            entry.reconciliation_status,
            SpecReconciliationStatusV1::Synchronized
                | SpecReconciliationStatusV1::OutstandingNonblockingFindings
        ) {
            let completed = spec_sync_queue::complete_for_origins(
                self.store,
                feature_uid,
                &entry.origin_events,
            )?;
            changes.extend(completed.changes);
            checks.extend(completed.checks);
        }
        if let Some(revision) = expected_store_revision {
            self.store.transaction_with_revision_and_record_revisions(
                &changes,
                Some(revision),
                &checks,
            )?;
        } else {
            self.store
                .transaction_with_record_revisions(&changes, &checks)?;
        }
        Ok(entry)
    }
}

pub(super) fn current_path(feature_uid: &str) -> Result<String, StoreError> {
    validation::require_uuid(feature_uid, "feature")?;
    Ok(format!("state/spec_revisions/{feature_uid}.json"))
}
