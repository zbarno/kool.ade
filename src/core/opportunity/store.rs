use crate::{
    artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError},
    harness::prompt_catalog::{
        OpportunityChoice, OpportunityDecisionV1 as UiDecisionV1,
        OpportunityProposalV1 as PromptProposalV1,
    },
};

use super::{
    OpportunityDecisionKindV1, OpportunityDecisionV1, OpportunityStatusV1, OpportunityV1,
    dedup_fingerprint,
};

mod decisions;
mod dedup;
mod helpers;
mod lifecycle;
mod prompt;
mod queries;
mod quota;
mod records;
mod settings;
use helpers::{dedup_path, encode, read_dedup};
use prompt::validate_prompt_projection;
use records::{
    OpportunityDecisionRecord, OpportunityDedupRecord, OpportunityRecord, load_record, now_unix_ms,
    opportunity_path, render_markdown,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpportunitySaveResult {
    Created {
        proposal: OpportunityV1,
        record_revision: u64,
    },
    Duplicate {
        proposal: OpportunityV1,
        record_revision: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpportunityDecisionResult {
    Recorded {
        proposal: OpportunityV1,
        record_revision: u64,
        decision: OpportunityDecisionV1,
    },
    AlreadyRecorded {
        proposal: OpportunityV1,
        record_revision: u64,
        decision: OpportunityDecisionV1,
    },
}

pub struct OpportunityStore<'a> {
    store: &'a PlanningStore,
}

impl<'a> OpportunityStore<'a> {
    pub fn new(store: &'a PlanningStore) -> Self {
        Self { store }
    }

    /// Read workspace-local exploration preferences. Missing state means
    /// exploration is disabled; enabling it always requires a persisted UI
    /// action through `save_discovery_settings`.
    pub fn discovery_settings(&self) -> Result<super::OpportunityDiscoverySettingsV1, StoreError> {
        settings::load(self.store)
    }

    pub fn save_discovery_settings(
        &self,
        settings: &super::OpportunityDiscoverySettingsV1,
        expected_revision: u64,
    ) -> Result<super::OpportunityDiscoverySettingsV1, StoreError> {
        settings::save(self.store, settings, expected_revision)
    }

    pub(crate) fn planning_store(&self) -> &PlanningStore {
        self.store
    }

    /// Persist an advisory proposal. The deduplication key is recomputed from
    /// normalized workspace/target/problem fields and committed atomically.
    pub fn save_advisory(
        &self,
        proposal: OpportunityV1,
    ) -> Result<OpportunitySaveResult, StoreError> {
        self.save_advisory_record(proposal, None)
    }

    /// Persist a Planner/Manager proposal alongside its normalized product
    /// record. The model fields remain advisory; this API cannot record a user
    /// decision or authorize planning.
    pub fn save_prompt_advisory(
        &self,
        proposal: OpportunityV1,
        prompt_proposal: PromptProposalV1,
    ) -> Result<OpportunitySaveResult, StoreError> {
        validate_prompt_projection(&proposal, &prompt_proposal)?;
        self.save_advisory_record(proposal, Some(prompt_proposal))
    }

    fn save_advisory_record(
        &self,
        proposal: OpportunityV1,
        prompt_proposal: Option<PromptProposalV1>,
    ) -> Result<OpportunitySaveResult, StoreError> {
        self.save_advisory_record_with_before_commit(proposal, prompt_proposal, || Ok(()))
    }

    pub(super) fn save_advisory_record_with_before_commit(
        &self,
        mut proposal: OpportunityV1,
        prompt_proposal: Option<PromptProposalV1>,
        before_commit: impl FnOnce() -> Result<(), StoreError>,
    ) -> Result<OpportunitySaveResult, StoreError> {
        proposal.dedup_fingerprint = dedup_fingerprint(
            &proposal.workspace_uid,
            &proposal.target.repository_uid,
            &proposal.problem_or_opportunity,
        )
        .map_err(StoreError::MalformedState)?;
        if !matches!(
            proposal.status,
            OpportunityStatusV1::Proposed | OpportunityStatusV1::AwaitingDecision
        ) || proposal.revision != 1
        {
            return Err(StoreError::MalformedState(
                "new opportunity must be an advisory proposal at revision one".into(),
            ));
        }
        proposal.status = OpportunityStatusV1::AwaitingDecision;
        proposal.validate().map_err(StoreError::MalformedState)?;
        let index_path = dedup_path(&proposal.dedup_fingerprint);
        let existing_index = read_dedup(self.store, &index_path)?;
        let existing_record = if let Some(index) = &existing_index {
            let record_path = opportunity_path(&index.opportunity_uid)?;
            Some(load_record(self.store, &record_path)?.ok_or_else(|| {
                StoreError::MalformedState(
                    "opportunity dedup record points to a missing proposal".into(),
                )
            })?)
        } else {
            None
        };
        if let (Some(index), Some(existing)) = (&existing_index, &existing_record) {
            if existing.data.dedup_fingerprint != index.uid {
                return Err(StoreError::MalformedState(
                    "opportunity dedup record points to a proposal with a different fingerprint"
                        .into(),
                ));
            }
            let newer_evidence = dedup::has_materially_new_evidence(
                &existing.data,
                existing.prompt_proposal.as_ref(),
                &proposal,
                prompt_proposal.as_ref(),
            );
            let can_reopen = match existing.data.status {
                OpportunityStatusV1::Declined => newer_evidence,
                OpportunityStatusV1::Deferred => {
                    let expired = self
                        .decision_for_opportunity(&existing.data.opportunity_uid)?
                        .as_ref()
                        .is_some_and(|decision| super::snooze_expired(decision, now_unix_ms()));
                    expired && newer_evidence
                }
                _ => false,
            };
            if !can_reopen {
                return Ok(OpportunitySaveResult::Duplicate {
                    proposal: existing.data.clone(),
                    record_revision: existing.revision,
                });
            }
            debug_assert_eq!(index.opportunity_uid, existing.uid);
        }
        let quota = quota::reserve(self.store, &proposal)?;
        let opportunity_path = opportunity_path(&proposal.opportunity_uid)?;
        if load_record(self.store, &opportunity_path)?.is_some() {
            return Err(StoreError::MalformedState(
                "opportunity identity already exists with a different dedup record".into(),
            ));
        }
        let now = now_unix_ms();
        let record = OpportunityRecord {
            schema_version: 1,
            uid: proposal.opportunity_uid.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            data: proposal.clone(),
            prompt_proposal,
        };
        let dedup = OpportunityDedupRecord {
            schema_version: 1,
            uid: proposal.dedup_fingerprint.clone(),
            created_at_ms: existing_index
                .as_ref()
                .map_or(now, |index| index.created_at_ms),
            updated_at_ms: now,
            revision: match existing_index.as_ref() {
                Some(index) => index.revision.checked_add(1).ok_or_else(|| {
                    StoreError::MalformedState("opportunity dedup revision overflow".into())
                })?,
                None => 1,
            },
            opportunity_uid: proposal.opportunity_uid.clone(),
        };
        let markdown_path = format!("planning/opportunities/{}.md", proposal.opportunity_uid);
        let changes = vec![
            (opportunity_path.clone(), encode(&record)?),
            (index_path.clone(), encode(&dedup)?),
            (markdown_path, render_markdown(&proposal).into_bytes()),
            (quota.path.clone(), quota.bytes),
        ];
        let mut checks = vec![
            RecordRevisionCheck {
                path: opportunity_path,
                expected_revision: 0,
            },
            RecordRevisionCheck {
                path: index_path,
                expected_revision: existing_index.as_ref().map_or(0, |index| index.revision),
            },
        ];
        if let Some(existing) = existing_record.as_ref().filter(|_| {
            existing_index
                .as_ref()
                .is_some_and(|index| index.opportunity_uid != proposal.opportunity_uid)
        }) {
            // Fence the declined/deferred proposal whose state authorized
            // reopening. A concurrent NotNow decision must invalidate this
            // transaction even though it does not write that older record.
            checks.push(RecordRevisionCheck {
                path: records::opportunity_path(&existing.uid)?,
                expected_revision: existing.revision,
            });
        }
        checks.push(quota.check);
        before_commit()?;
        match self
            .store
            .transaction_with_record_revisions(&changes, &checks)
        {
            Ok(_) => Ok(OpportunitySaveResult::Created {
                proposal,
                record_revision: 1,
            }),
            Err(StoreError::StaleRecordRevision {
                path,
                expected,
                actual,
            }) => {
                if let Some(index) =
                    read_dedup(self.store, &dedup_path(&record.data.dedup_fingerprint))?
                {
                    let (existing, revision) = self.load(&index.opportunity_uid)?;
                    Ok(OpportunitySaveResult::Duplicate {
                        proposal: existing,
                        record_revision: revision,
                    })
                } else {
                    Err(StoreError::StaleRecordRevision {
                        path,
                        expected,
                        actual,
                    })
                }
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) fn decision_status(
    choice: &OpportunityChoice,
) -> (OpportunityDecisionKindV1, OpportunityStatusV1) {
    match choice {
        OpportunityChoice::Pursue => (
            OpportunityDecisionKindV1::Pursue,
            OpportunityStatusV1::ApprovedToPlan,
        ),
        OpportunityChoice::NotNow => (
            OpportunityDecisionKindV1::NotNow,
            OpportunityStatusV1::Deferred,
        ),
        OpportunityChoice::Decline => (
            OpportunityDecisionKindV1::Decline,
            OpportunityStatusV1::Declined,
        ),
        OpportunityChoice::AskForMoreInformation => (
            OpportunityDecisionKindV1::AskForMoreInformation,
            OpportunityStatusV1::InformationRequested,
        ),
    }
}
