//! Evidence-backed improvement proposals. Proposals are advisory; only a
//! user-authored decision can authorize entering the existing planning flow.

use serde::{Deserialize, Serialize};
use validation::*;

mod settings;
mod store;
mod validation;
pub use settings::{OpportunityDiscoverySettingsV1, OpportunityExploreModeV1};
pub use store::{OpportunityDecisionResult, OpportunitySaveResult, OpportunityStore};

#[cfg(test)]
#[path = "opportunity/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityOriginV1 {
    UserGoal,
    RunFailure,
    QaFinding,
    ReviewFinding,
    SpecDrift,
    CodeInspection,
    UserFeedback,
    Roadmap,
    ScheduledExploration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityKindV1 {
    NewFeature,
    ExistingFeatureExtension,
    BugFix,
    Integration,
    TechStackSupport,
    Quality,
    Security,
    Performance,
    DeveloperExperience,
    Documentation,
    Infrastructure,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityTargetKindV1 {
    ActiveUserProject,
    KooladeProduct,
    OtherLinkedRepository,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityStatusV1 {
    Draft,
    Proposed,
    AwaitingDecision,
    InformationRequested,
    ApprovedToPlan,
    Declined,
    Deferred,
    Superseded,
    Planning,
    Implemented,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityScopeV1 {
    Small,
    Medium,
    Large,
    Unknown,
}

/// Identifies a concrete linked repository without embedding source or logs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityTargetV1 {
    pub kind: OpportunityTargetKindV1,
    pub repository_uid: String,
    pub canonical_remote: String,
    pub source_sha: String,
}

/// Evidence is referenced by stable ID and digest. Confidential evidence stays
/// in the user's local planning store and is never copied into a public proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityEvidenceRefV1 {
    pub evidence_uid: String,
    pub sha256: String,
    pub confidentiality: EvidenceConfidentialityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidentialityV1 {
    Public,
    UserLocal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpportunityV1 {
    pub schema_version: u16,
    pub opportunity_uid: String,
    pub workspace_uid: String,
    pub origin: OpportunityOriginV1,
    pub kind: OpportunityKindV1,
    pub target: OpportunityTargetV1,
    pub related_feature_uids: Vec<String>,
    pub related_task_uids: Vec<String>,
    pub discovered_at_ms: u64,
    pub observation_revision: u64,
    pub evidence_refs: Vec<OpportunityEvidenceRefV1>,
    pub problem_or_opportunity: String,
    pub user_value: String,
    pub expected_outcome: String,
    pub possible_approaches: Vec<String>,
    pub recommended_approach: String,
    pub alternatives: Vec<String>,
    pub estimated_scope: OpportunityScopeV1,
    pub impact: String,
    pub cost_or_risk: String,
    pub affected_systems: Vec<String>,
    pub current_capability_state: String,
    pub generic_workaround: Option<String>,
    pub proposed_approach: String,
    pub dedup_fingerprint: String,
    pub status: OpportunityStatusV1,
    pub revision: u64,
}

impl OpportunityV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.opportunity_uid)
            || !is_uuid(&self.workspace_uid)
            || !is_uuid(&self.target.repository_uid)
            || !is_git_object_id(&self.target.source_sha)
            || !is_sha256(&self.dedup_fingerprint)
            || self.target.canonical_remote.trim().is_empty()
            || self.target.canonical_remote.len() > 2048
            || self.revision == 0
            || self.observation_revision == 0
            || self.evidence_refs.len() > 64
            || self.related_feature_uids.len() > 256
            || self.related_task_uids.len() > 512
            || self
                .related_feature_uids
                .iter()
                .chain(&self.related_task_uids)
                .any(|id| !is_uuid(id))
            || self
                .evidence_refs
                .iter()
                .any(|item| !is_uuid(&item.evidence_uid) || !is_sha256(&item.sha256))
            || !valid_list(&self.possible_approaches, 16, 2048)
            || !valid_list(&self.alternatives, 16, 2048)
            || !valid_list(&self.affected_systems, 64, 256)
            || [
                (&self.problem_or_opportunity, 4096),
                (&self.user_value, 2048),
                (&self.expected_outcome, 2048),
                (&self.recommended_approach, 2048),
                (&self.impact, 2048),
                (&self.cost_or_risk, 2048),
                (&self.current_capability_state, 1024),
                (&self.proposed_approach, 2048),
            ]
            .into_iter()
            .any(|(value, limit)| !valid_text(value, limit))
            || self
                .generic_workaround
                .as_ref()
                .is_some_and(|value| !valid_text(value, 2048))
        {
            return Err("opportunity proposal is invalid".into());
        }
        if self.target.kind == OpportunityTargetKindV1::KooladeProduct
            && !is_koolade_remote(&self.target.canonical_remote)
        {
            return Err(
                "Kool.ad/e opportunities must target the canonical product repository".into(),
            );
        }
        if self.target.kind != OpportunityTargetKindV1::ActiveUserProject
            && self
                .evidence_refs
                .iter()
                .any(|item| item.confidentiality != EvidenceConfidentialityV1::Public)
        {
            return Err("cross-repository opportunities cannot cite user-local evidence".into());
        }
        Ok(())
    }

    pub fn revision_hash(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(hex_digest(&bytes))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityDecisionKindV1 {
    Pursue,
    NotNow,
    Decline,
    AskForMoreInformation,
}

/// Default duration of the board's user-facing "Not now" snooze.
pub const OPPORTUNITY_SNOOZE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub fn snooze_expired(decision: &OpportunityDecisionV1, now_ms: u64) -> bool {
    decision.decision == OpportunityDecisionKindV1::NotNow
        && decision
            .snooze_until_ms
            .is_some_and(|until| now_ms >= until)
}

/// A decision receipt is application-created only after a UI action by a
/// verified user. Model output has no API that can construct this receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpportunityDecisionV1 {
    pub schema_version: u16,
    pub uid: String,
    pub opportunity_uid: String,
    pub opportunity_revision_hash: String,
    pub user_id: String,
    pub decision: OpportunityDecisionKindV1,
    pub occurred_at_ms: u64,
    #[serde(default)]
    pub snooze_until_ms: Option<u64>,
    pub explanation: Option<String>,
}

impl OpportunityDecisionV1 {
    pub fn validate(&self, proposal: &OpportunityV1) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.uid)
            || self.opportunity_uid != proposal.opportunity_uid
            || self.opportunity_revision_hash != proposal.revision_hash()?
            || self.user_id.trim().is_empty()
            || self.user_id.len() > 256
            || self.occurred_at_ms == 0
            || match (&self.decision, self.snooze_until_ms) {
                (OpportunityDecisionKindV1::NotNow, Some(until)) => {
                    until <= self.occurred_at_ms
                        || until > self.occurred_at_ms.saturating_add(90 * 24 * 60 * 60 * 1000)
                }
                (OpportunityDecisionKindV1::NotNow, None) => true,
                (_, Some(_)) => true,
                (_, None) => false,
            }
            || self
                .explanation
                .as_ref()
                .is_some_and(|value| !valid_text(value, 2048))
            || (proposal.status != OpportunityStatusV1::AwaitingDecision
                && !(proposal.status == OpportunityStatusV1::InformationRequested
                    && self.decision != OpportunityDecisionKindV1::AskForMoreInformation)
                && !(proposal.status == OpportunityStatusV1::Deferred
                    && self.decision != OpportunityDecisionKindV1::AskForMoreInformation))
        {
            return Err("opportunity decision is stale or invalid".into());
        }
        Ok(())
    }
}

pub fn dedup_fingerprint(
    workspace_uid: &str,
    target_uid: &str,
    normalized_problem: &str,
) -> Result<String, String> {
    if !is_uuid(workspace_uid) || !is_uuid(target_uid) || !valid_text(normalized_problem, 4096) {
        return Err("opportunity fingerprint inputs are invalid".into());
    }
    let normalized = normalized_problem
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    Ok(hex_digest(
        format!("v1\0{workspace_uid}\0{target_uid}\0{normalized}").as_bytes(),
    ))
}
