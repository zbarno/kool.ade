use serde::{Deserialize, Serialize};

mod active_runs;
mod records;
#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
mod transitions;
use records::{
    canonical_uuid, encode, now_unix_ms, run_path, transition_path, validate_run_record,
};

use crate::{
    artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError},
    core::opportunity::{
        OpportunityDecisionKindV1, OpportunityStatusV1, OpportunityStore, OpportunityTargetKindV1,
    },
};

use super::{SelfImprovementEventV1, SelfImprovementPhaseV1, SelfImprovementRunV1};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRecord {
    schema_version: u16,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    data: SelfImprovementRunV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelfImprovementTransitionV1 {
    pub schema_version: u16,
    pub uid: String,
    pub run_uid: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub from_phase: SelfImprovementPhaseV1,
    pub to_phase: SelfImprovementPhaseV1,
    pub event: Option<SelfImprovementEventV1>,
    pub opportunity_decision_uid: Option<String>,
}

/// Persists self-improvement progress alongside the ordinary planning records.
/// Callers must verify user/reviewer/runtime receipts before applying events.
pub struct SelfImprovementStore<'a> {
    store: &'a PlanningStore,
}

impl<'a> SelfImprovementStore<'a> {
    pub fn new(store: &'a PlanningStore) -> Self {
        Self { store }
    }

    /// Create one deterministic run from a persisted Pursue receipt. Repeating
    /// the call for the same decision returns that run without duplicating state.
    pub fn create_from_pursued(
        &self,
        opportunities: &OpportunityStore<'_>,
        decision_uid: &str,
        mut run: SelfImprovementRunV1,
    ) -> Result<SelfImprovementRunV1, StoreError> {
        if self.store.project_id != opportunities.planning_store().project_id
            || self.store.root != opportunities.planning_store().root
            || self.store.mode != opportunities.planning_store().mode
        {
            return Err(StoreError::MalformedState(
                "opportunity and self-improvement records must share one planning store".into(),
            ));
        }
        let decision = opportunities.decision(decision_uid)?;
        if decision.decision != OpportunityDecisionKindV1::Pursue
            || decision.uid != run.approved_decision_uid
            || decision.opportunity_uid != run.opportunity_uid
        {
            return Err(StoreError::MalformedState(
                "self-improvement requires this run's persisted Pursue decision".into(),
            ));
        }
        let (proposal, record_revision) = opportunities.load(&decision.opportunity_uid)?;
        let already_linked = proposal.status == OpportunityStatusV1::Planning
            && proposal.related_feature_uids.contains(&run.feature_uid);
        if (proposal.status != OpportunityStatusV1::ApprovedToPlan && !already_linked)
            || proposal.target.kind != OpportunityTargetKindV1::KooladeProduct
            || proposal.target.repository_uid != run.target_repository_uid
            || proposal.target.canonical_remote != run.canonical_remote
            || proposal.target.source_sha != run.source_sha
            || run.phase != SelfImprovementPhaseV1::ApprovedToPlan
            || run.revision != 0
        {
            return Err(StoreError::MalformedState(
                "Pursue receipt does not authorize this Kool.ad/e source target".into(),
            ));
        }
        let expected_uid = Self::run_uid_for_decision(decision_uid)?;
        if run.run_uid != expected_uid {
            return Err(StoreError::MalformedState(
                "self-improvement identity must be derived from its Pursue decision".into(),
            ));
        }
        run.revision = 1;
        run.validate().map_err(StoreError::MalformedState)?;
        let now = now_unix_ms();
        let event_uid = Self::created_event_uid(decision_uid)?;
        let transition = SelfImprovementTransitionV1 {
            schema_version: 1,
            uid: event_uid.clone(),
            run_uid: run.run_uid.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            from_phase: SelfImprovementPhaseV1::ApprovedToPlan,
            to_phase: SelfImprovementPhaseV1::ApprovedToPlan,
            event: None,
            opportunity_decision_uid: Some(decision_uid.to_owned()),
        };
        let active_limit = opportunities
            .discovery_settings()?
            .active_self_improvement_limit;
        let mut created = false;
        for attempt in 0..8 {
            let slot = active_runs::reserve(self.store, &run.run_uid, active_limit)?;
            match self.create_record(&run, &transition, slot.as_ref()) {
                Ok(()) => {
                    created = true;
                    break;
                }
                Err(error @ StoreError::StaleRecordRevision { .. }) => {
                    match self.load(&run.run_uid) {
                        Ok(existing) => {
                            if existing.opportunity_uid != run.opportunity_uid
                                || existing.approved_decision_uid != run.approved_decision_uid
                            {
                                return Err(StoreError::MalformedState(
                                    "self-improvement run identity is already bound to another decision".into(),
                                ));
                            }
                            run = existing;
                            break;
                        }
                        Err(StoreError::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound && attempt < 7 =>
                        {
                            continue;
                        }
                        Err(StoreError::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound =>
                        {
                            return Err(error);
                        }
                        Err(load_error) => return Err(load_error),
                    }
                }
                Err(error) => return Err(error),
            }
        }
        if !created && run.revision == 1 {
            // An existing durable record was loaded after an idempotent replay.
            let persisted = self.load(&run.run_uid)?;
            run = persisted;
        }
        if !already_linked {
            match opportunities.mark_planning(
                &proposal.opportunity_uid,
                &run.feature_uid,
                record_revision,
            ) {
                Ok(_) => {}
                Err(StoreError::StaleRecordRevision { .. }) => {
                    let (current, _) = opportunities.load(&proposal.opportunity_uid)?;
                    if current.status != OpportunityStatusV1::Planning
                        || !current.related_feature_uids.contains(&run.feature_uid)
                    {
                        return Err(StoreError::MalformedState(
                            "opportunity changed before its planning run was linked".into(),
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
        let event_uid = Self::planning_event_uid(decision_uid)?;
        self.apply_event(
            &run.run_uid,
            run.revision,
            &event_uid,
            SelfImprovementEventV1::BeginPlanning,
        )
    }

    pub fn load(&self, uid: &str) -> Result<SelfImprovementRunV1, StoreError> {
        let path = run_path(uid)?;
        let (record, actual_revision) = self.store.read_record::<RunRecord>(&path)?;
        validate_run_record(&record, actual_revision, uid, &path)?;
        Ok(record.data)
    }

    pub fn run_uid_for_decision(decision_uid: &str) -> Result<String, StoreError> {
        canonical_uuid(decision_uid, "opportunity decision")?;
        Ok(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("https://github.com/zbarno/kool.ade/self-improvement/{decision_uid}")
                .as_bytes(),
        )
        .hyphenated()
        .to_string())
    }

    fn created_event_uid(decision_uid: &str) -> Result<String, StoreError> {
        canonical_uuid(decision_uid, "opportunity decision")?;
        Ok(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("https://github.com/zbarno/kool.ade/self-improvement/{decision_uid}/created")
                .as_bytes(),
        )
        .hyphenated()
        .to_string())
    }

    fn planning_event_uid(decision_uid: &str) -> Result<String, StoreError> {
        canonical_uuid(decision_uid, "opportunity decision")?;
        Ok(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("https://github.com/zbarno/kool.ade/self-improvement/{decision_uid}/planning")
                .as_bytes(),
        )
        .hyphenated()
        .to_string())
    }

    fn create_record(
        &self,
        run: &SelfImprovementRunV1,
        transition: &SelfImprovementTransitionV1,
        active_slot: Option<&active_runs::Change>,
    ) -> Result<(), StoreError> {
        let path = run_path(&run.run_uid)?;
        let event_path = transition_path(&run.run_uid, &transition.uid)?;
        let now = now_unix_ms();
        let record = RunRecord {
            schema_version: 1,
            uid: run.run_uid.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            revision: run.revision,
            data: run.clone(),
        };
        let mut changes = vec![
            (path.clone(), encode(&record)?),
            (event_path.clone(), encode(transition)?),
        ];
        let mut checks = vec![
            RecordRevisionCheck {
                path,
                expected_revision: 0,
            },
            RecordRevisionCheck {
                path: event_path,
                expected_revision: 0,
            },
        ];
        if let Some(active_slot) = active_slot {
            changes.push((active_slot.path.clone(), active_slot.bytes.clone()));
            checks.push(active_slot.check.clone());
        }
        self.store
            .transaction_with_record_revisions(&changes, &checks)
            .map(|_| ())
    }
}
