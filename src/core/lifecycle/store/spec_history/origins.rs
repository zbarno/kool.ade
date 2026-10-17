use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        SpecDeltaReferenceV1, SpecOriginDispositionV1, SpecOriginOutcomeV1,
        SpecReconciliationStatusV1, SpecRevisionDraftV1,
    },
};

use super::{LifecycleStore, SpecRevisionHistory, validation};

pub(super) fn for_draft(
    draft: &SpecRevisionDraftV1,
) -> Result<Vec<SpecOriginDispositionV1>, StoreError> {
    let outcome = if !draft.deltas.is_empty() {
        SpecOriginOutcomeV1::DeltaReferences {
            deltas: draft
                .deltas
                .iter()
                .map(|delta| SpecDeltaReferenceV1 {
                    event_uid: delta.event_id.clone(),
                    requirement_id: delta.requirement_id.clone(),
                })
                .collect(),
        }
    } else if matches!(
        draft.reconciliation_status,
        SpecReconciliationStatusV1::BlockingDrift | SpecReconciliationStatusV1::NeedsHumanDecision
    ) {
        SpecOriginOutcomeV1::Unresolved {
            reason: require_reason(draft.outcome_reason.as_deref())?.to_owned(),
        }
    } else {
        SpecOriginOutcomeV1::NoChange {
            reason: require_reason(draft.no_change_reason.as_deref())?.to_owned(),
        }
    };
    Ok(draft
        .origin_events
        .iter()
        .map(|uid| SpecOriginDispositionV1 {
            origin_event_uid: uid.clone(),
            outcome: outcome.clone(),
        })
        .collect())
}

fn require_reason(reason: Option<&str>) -> Result<&str, StoreError> {
    reason
        .filter(|reason| {
            !reason.trim().is_empty() && reason.len() <= 4096 && !reason.contains('\0')
        })
        .ok_or_else(|| {
            StoreError::MalformedState(
                "specification origin disposition requires a bounded explicit reason".into(),
            )
        })
}

pub(super) fn draft_matches(draft: &SpecRevisionDraftV1, history: &SpecRevisionHistory) -> bool {
    let entry = &history.entry;
    draft.content_hash == entry.content_hash
        && draft.source_revision == entry.source_revision
        && draft.produced_by_run_id == entry.produced_by_run_id
        && draft.origin_events == entry.origin_events
        && draft.decision_refs == entry.decision_refs
        && draft.approved_target_hash == entry.approved_target_hash
        && draft.implementation_head_sha == entry.implementation_head_sha
        && draft.reconciliation_status == entry.reconciliation_status
        && draft.outcome_reason == entry.outcome_reason
        && draft.deltas == history.deltas
        && history.origin_dispositions.as_ref().is_none_or(|stored| {
            for_draft(draft)
                .as_ref()
                .is_ok_and(|proposed| proposed == stored)
        })
}

pub(super) fn validate(history: &SpecRevisionHistory) -> Result<(), StoreError> {
    let Some(dispositions) = &history.origin_dispositions else {
        return Ok(());
    };
    let no_change_reason = dispositions
        .iter()
        .find_map(|disposition| match &disposition.outcome {
            SpecOriginOutcomeV1::NoChange { reason } => Some(reason.clone()),
            _ => None,
        });
    let draft = SpecRevisionDraftV1 {
        content_hash: history.entry.content_hash.clone(),
        source_revision: history.entry.source_revision,
        produced_by_run_id: history.entry.produced_by_run_id.clone(),
        origin_events: history.entry.origin_events.clone(),
        decision_refs: history.entry.decision_refs.clone(),
        delta_approval_refs: Vec::new(),
        approved_target_hash: history.entry.approved_target_hash.clone(),
        implementation_head_sha: history.entry.implementation_head_sha.clone(),
        reconciliation_status: history.entry.reconciliation_status,
        outcome_reason: history.entry.outcome_reason.clone(),
        no_change_reason,
        deltas: history.deltas.clone(),
    };
    if dispositions != &for_draft(&draft)? {
        return Err(StoreError::MalformedState(
            "specification origin dispositions do not match their revision and deltas".into(),
        ));
    }
    Ok(())
}

impl LifecycleStore<'_> {
    /// Load the recorded dispositions of an exact immutable revision. None
    /// means legacy history without explicit outcomes, never an inferred pass.
    pub fn specification_origin_dispositions(
        &self,
        feature_uid: &str,
        sequence: u64,
    ) -> Result<Option<Vec<SpecOriginDispositionV1>>, StoreError> {
        let uid = validation::revision_uid(feature_uid, sequence)?;
        let (history, revision) = self.store.read_record::<SpecRevisionHistory>(&format!(
            "state/spec_revision_history/{uid}.json"
        ))?;
        validation::validate_history(&history, revision)?;
        if history.feature_uid != feature_uid || history.sequence != sequence {
            return Err(StoreError::MalformedState(
                "specification origin history identity mismatch".into(),
            ));
        }
        Ok(history.origin_dispositions)
    }
}
