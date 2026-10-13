use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::opportunity::{OpportunityDecisionV1, OpportunityStatusV1},
    harness::prompt_catalog::{
        OpportunityDecisionV1 as UiDecisionV1, validate_user_opportunity_decision,
    },
};

use super::helpers::encode;
use super::{
    OpportunityDecisionRecord, OpportunityDecisionResult, OpportunityRecord, OpportunityStore,
    now_unix_ms, opportunity_path, render_markdown,
};

pub(super) fn record_user_decision(
    store: &OpportunityStore<'_>,
    expected_record_revision: u64,
    decision: &UiDecisionV1,
    authenticated_user_id: &str,
) -> Result<OpportunityDecisionResult, StoreError> {
    record_user_decision_at(
        store,
        expected_record_revision,
        decision,
        authenticated_user_id,
        now_unix_ms(),
    )
}

pub(super) fn record_user_decision_at(
    store: &OpportunityStore<'_>,
    expected_record_revision: u64,
    decision: &UiDecisionV1,
    authenticated_user_id: &str,
    now_ms: u64,
) -> Result<OpportunityDecisionResult, StoreError> {
    let decision_path = super::records::decision_path(&decision.uid)?;
    match store
        .store
        .read_record::<OpportunityDecisionRecord>(&decision_path)
    {
        Ok((existing, revision)) => {
            if existing.schema_version == 1
                && existing.uid == decision.uid
                && existing.revision == revision
                && serde_json::to_vec(&existing.prompt_decision).ok()
                    == serde_json::to_vec(decision).ok()
                && existing.prompt_decision.user_id == authenticated_user_id
            {
                let (proposal, proposal_revision) = store.load(&existing.data.opportunity_uid)?;
                return Ok(OpportunityDecisionResult::AlreadyRecorded {
                    proposal,
                    record_revision: proposal_revision,
                    decision: existing.data,
                });
            }
            return Err(StoreError::MalformedState(
                "decision UID already belongs to a different or malformed receipt".into(),
            ));
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let (mut proposal, actual_record_revision) = store.load(&decision.opportunity_uid)?;
    if actual_record_revision != expected_record_revision {
        return Err(StoreError::StaleRecordRevision {
            path: opportunity_path(&decision.opportunity_uid)?,
            expected: expected_record_revision,
            actual: actual_record_revision,
        });
    }
    let expected_hash = proposal
        .revision_hash()
        .map_err(StoreError::MalformedState)?;
    validate_user_opportunity_decision(
        decision,
        &proposal.opportunity_uid,
        &expected_hash,
        authenticated_user_id,
    )
    .map_err(StoreError::MalformedState)?;
    let follow_up_after_question = proposal.status == OpportunityStatusV1::InformationRequested
        && decision.decision
            != crate::harness::prompt_catalog::OpportunityChoice::AskForMoreInformation;
    let follow_up_after_snooze = proposal.status == OpportunityStatusV1::Deferred
        && store
            .decision_for_opportunity(&proposal.opportunity_uid)?
            .as_ref()
            .is_some_and(|decision| crate::core::opportunity::snooze_expired(decision, now_ms));
    if proposal.status != OpportunityStatusV1::AwaitingDecision
        && !follow_up_after_question
        && !follow_up_after_snooze
    {
        return Err(StoreError::MalformedState(
            "opportunity is not awaiting an explicit user decision".into(),
        ));
    }

    let (decision_kind, next_status) = super::decision_status(&decision.decision);
    let occurred_at_ms = chrono::DateTime::parse_from_rfc3339(&decision.occurred_at)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        .timestamp_millis()
        .try_into()
        .map_err(|_| StoreError::MalformedState("decision timestamp is out of range".into()))?;
    let receipt = OpportunityDecisionV1 {
        schema_version: 1,
        uid: decision.uid.clone(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        opportunity_revision_hash: expected_hash,
        user_id: authenticated_user_id.to_owned(),
        decision: decision_kind,
        occurred_at_ms,
        snooze_until_ms: decision
            .snooze_until
            .as_deref()
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map_err(|error| StoreError::MalformedState(error.to_string()))?
                    .timestamp_millis()
                    .try_into()
                    .map_err(|_| StoreError::MalformedState("snooze expiry is out of range".into()))
            })
            .transpose()?,
        explanation: decision.explanation.clone(),
    };
    receipt
        .validate(&proposal)
        .map_err(StoreError::MalformedState)?;

    proposal.status = next_status;
    proposal.revision = proposal
        .revision
        .checked_add(1)
        .ok_or_else(|| StoreError::MalformedState("opportunity revision overflow".into()))?;
    let now = now_unix_ms();
    let path = opportunity_path(&proposal.opportunity_uid)?;
    let (current_record, _) = store.store.read_record::<OpportunityRecord>(&path)?;
    let proposal_record = OpportunityRecord {
        schema_version: 1,
        uid: proposal.opportunity_uid.clone(),
        created_at_ms: current_record.created_at_ms,
        updated_at_ms: now,
        revision: actual_record_revision + 1,
        data: proposal.clone(),
        prompt_proposal: current_record.prompt_proposal,
    };
    let decision_record = OpportunityDecisionRecord {
        schema_version: 1,
        uid: decision.uid.clone(),
        created_at_ms: now,
        updated_at_ms: now,
        revision: 1,
        data: receipt.clone(),
        prompt_decision: decision.clone(),
    };
    let opportunity_path = opportunity_path(&proposal.opportunity_uid)?;
    let decision_path = super::records::decision_path(&decision.uid)?;
    let markdown_path = format!("planning/opportunities/{}.md", proposal.opportunity_uid);
    let changes = vec![
        (opportunity_path.clone(), encode(&proposal_record)?),
        (decision_path.clone(), encode(&decision_record)?),
        (markdown_path, render_markdown(&proposal).into_bytes()),
    ];
    let checks = vec![
        RecordRevisionCheck {
            path: opportunity_path,
            expected_revision: actual_record_revision,
        },
        RecordRevisionCheck {
            path: decision_path,
            expected_revision: 0,
        },
    ];
    store
        .store
        .transaction_with_record_revisions(&changes, &checks)?;
    Ok(OpportunityDecisionResult::Recorded {
        proposal,
        record_revision: actual_record_revision + 1,
        decision: receipt,
    })
}

pub(super) fn load_decision(
    store: &crate::artifacts::planning_store::PlanningStore,
    uid: &str,
) -> Result<OpportunityDecisionV1, StoreError> {
    super::records::load_decision(store, uid)
}

impl OpportunityStore<'_> {
    pub fn decision_for_opportunity(
        &self,
        opportunity_uid: &str,
    ) -> Result<Option<OpportunityDecisionV1>, StoreError> {
        let matching = self
            .store
            .list_record_paths("state/opportunity_decisions")?
            .into_iter()
            .map(|path| {
                let uid = path
                    .strip_prefix("state/opportunity_decisions/")
                    .and_then(|name| name.strip_suffix(".json"))
                    .ok_or_else(|| {
                        StoreError::MalformedState("invalid decision record path".into())
                    })?;
                load_decision(self.store, uid)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|decision| decision.opportunity_uid == opportunity_uid);
        Ok(matching.max_by_key(|decision| (decision.occurred_at_ms, decision.uid.clone())))
    }

    pub fn list_decisions(&self) -> Result<Vec<OpportunityDecisionV1>, StoreError> {
        let mut decisions = self
            .store
            .list_record_paths("state/opportunity_decisions")?
            .into_iter()
            .map(|path| {
                let uid = path
                    .strip_prefix("state/opportunity_decisions/")
                    .and_then(|name| name.strip_suffix(".json"))
                    .ok_or_else(|| {
                        StoreError::MalformedState("invalid decision record path".into())
                    })?;
                load_decision(self.store, uid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        decisions.sort_by_key(|decision| (decision.occurred_at_ms, decision.uid.clone()));
        Ok(decisions)
    }
}
