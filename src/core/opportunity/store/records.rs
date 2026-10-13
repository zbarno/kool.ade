use serde::{Deserialize, Serialize};

use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::opportunity::{OpportunityDecisionV1, OpportunityV1},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpportunityRecord {
    pub schema_version: u16,
    pub uid: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub data: OpportunityV1,
    #[serde(default)]
    pub prompt_proposal: Option<crate::harness::prompt_catalog::OpportunityProposalV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpportunityDedupRecord {
    pub schema_version: u16,
    pub uid: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub opportunity_uid: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpportunityDecisionRecord {
    pub schema_version: u16,
    pub uid: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub data: OpportunityDecisionV1,
    pub prompt_decision: crate::harness::prompt_catalog::OpportunityDecisionV1,
}

pub(super) fn opportunity_path(uid: &str) -> Result<String, StoreError> {
    let parsed = uuid::Uuid::parse_str(uid)
        .map_err(|_| StoreError::MalformedState("invalid opportunity identity".into()))?;
    if parsed.hyphenated().to_string() != uid {
        return Err(StoreError::MalformedState(
            "opportunity identity is not canonical".into(),
        ));
    }
    Ok(format!("state/opportunities/{uid}.json"))
}

pub(super) fn decision_path(uid: &str) -> Result<String, StoreError> {
    let parsed = uuid::Uuid::parse_str(uid)
        .map_err(|_| StoreError::MalformedState("invalid decision identity".into()))?;
    if parsed.hyphenated().to_string() != uid {
        return Err(StoreError::MalformedState(
            "decision identity is not canonical".into(),
        ));
    }
    Ok(format!("state/opportunity_decisions/{uid}.json"))
}

pub(super) fn load_record(
    store: &PlanningStore,
    path: &str,
) -> Result<Option<OpportunityRecord>, StoreError> {
    match store.read_record::<OpportunityRecord>(path) {
        Ok((record, actual_revision)) => {
            let expected_uid = path
                .strip_prefix("state/opportunities/")
                .and_then(|name| name.strip_suffix(".json"))
                .unwrap_or_default();
            if record.schema_version != 1
                || record.revision != actual_revision
                || record.uid != expected_uid
                || record.data.opportunity_uid != expected_uid
            {
                return Err(StoreError::MalformedState(format!(
                    "opportunity record is inconsistent: {path}"
                )));
            }
            record.data.validate().map_err(StoreError::MalformedState)?;
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn load_decision(
    store: &PlanningStore,
    uid: &str,
) -> Result<OpportunityDecisionV1, StoreError> {
    let path = decision_path(uid)?;
    let (record, actual_revision) = store.read_record::<OpportunityDecisionRecord>(&path)?;
    if record.schema_version != 1
        || record.uid != uid
        || record.revision != actual_revision
        || record.data.uid != uid
    {
        return Err(StoreError::MalformedState(format!(
            "opportunity decision record is inconsistent: {path}"
        )));
    }
    Ok(record.data)
}

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn render_markdown(proposal: &OpportunityV1) -> String {
    format!(
        "# Opportunity: {}\n\n- Status: `{:?}`\n- Revision: {}\n- Target repository: `{}`\n- Source revision: `{}`\n- Origin: `{:?}`\n- Kind: `{:?}`\n\n## Problem or opportunity\n\n{}\n\n## User value\n\n{}\n\n## Expected outcome\n\n{}\n\n## Proposed approach\n\n{}\n\n## Evidence references\n\n{}\n",
        proposal.opportunity_uid,
        proposal.status,
        proposal.revision,
        proposal.target.repository_uid,
        proposal.target.source_sha,
        proposal.origin,
        proposal.kind,
        proposal.problem_or_opportunity,
        proposal.user_value,
        proposal.expected_outcome,
        proposal.proposed_approach,
        proposal
            .evidence_refs
            .iter()
            .map(|evidence| format!(
                "- `{}` (sha256 `{}`; {:?})",
                evidence.evidence_uid, evidence.sha256, evidence.confidentiality
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
