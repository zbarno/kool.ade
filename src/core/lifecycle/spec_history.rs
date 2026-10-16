use serde::{Deserialize, Serialize};

use super::types::{is_git_oid, is_sha256, is_uuid};
mod delta;
mod product;
pub use delta::*;
pub use product::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecReconciliationStatusV1 {
    Synchronized,
    OutstandingNonblockingFindings,
    BlockingDrift,
    NeedsHumanDecision,
}

/// The latest durable per-feature projection. Immutable historical copies are
/// stored separately for each sequence number.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecRevisionV1 {
    pub schema_version: u16,
    pub uid: String,
    pub feature_uid: String,
    /// PlanningStore record revision; equal to `seq` for this projection.
    pub revision: u64,
    pub seq: u64,
    pub content_hash: String,
    pub source_revision: u64,
    pub produced_by_run_id: Option<String>,
    pub origin_events: Vec<String>,
    pub decision_refs: Vec<String>,
    pub approved_target_hash: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub reconciliation_status: SpecReconciliationStatusV1,
    #[serde(default)]
    pub outcome_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecRevisionDraftV1 {
    pub content_hash: String,
    pub source_revision: u64,
    pub produced_by_run_id: Option<String>,
    pub origin_events: Vec<String>,
    pub decision_refs: Vec<String>,
    pub delta_approval_refs: Vec<SpecDeltaApprovalRefV1>,
    pub approved_target_hash: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub reconciliation_status: SpecReconciliationStatusV1,
    pub outcome_reason: Option<String>,
    pub deltas: Vec<SpecDeltaRecordV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecDeltaApprovalRefV1 {
    pub task_uid: String,
    pub proposal_sha256: String,
}

impl SpecRevisionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.feature_uid)
            || self.uid != self.feature_uid
            || self.revision == 0
            || self.seq != self.revision
            || !is_sha256(&self.content_hash)
            || self.source_revision == 0
            || self
                .produced_by_run_id
                .as_deref()
                .is_some_and(|run| !is_uuid(run))
            || self.origin_events.is_empty()
            || self.origin_events.len() > 512
            || self.origin_events.iter().any(|event| !is_uuid(event))
            || has_duplicates(&self.origin_events)
            || self.decision_refs.len() > 256
            || self.decision_refs.iter().any(|decision| !is_uuid(decision))
            || has_duplicates(&self.decision_refs)
            || self
                .approved_target_hash
                .as_deref()
                .is_some_and(|hash| !is_sha256(hash))
            || self
                .implementation_head_sha
                .as_deref()
                .is_some_and(|sha| !is_git_oid(sha))
            || self.created_at_ms == 0
            || self.updated_at_ms < self.created_at_ms
            || self
                .outcome_reason
                .as_deref()
                .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 4096)
            || matches!(
                self.reconciliation_status,
                SpecReconciliationStatusV1::BlockingDrift
                    | SpecReconciliationStatusV1::NeedsHumanDecision
            ) != self.outcome_reason.is_some()
        {
            return Err("specification revision is invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSyncTriggerKindV1 {
    FeatureClarification,
    UserDecision,
    PlanReview,
    TaskGeneration,
    Implementation,
    CodeReview,
    RuntimeQa,
    Repair,
    SpecificationReconciliation,
    PullRequest,
    LifecycleOther,
    ManualRefresh,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSyncQueueStatusV1 {
    Pending,
    Completed,
    Declined,
}

/// Event-ID keyed queue projection. Queue records are inserted in the same
/// PlanningStore transaction as their lifecycle source event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecSyncQueueEntryV1 {
    pub schema_version: u16,
    pub uid: String,
    pub feature_uid: String,
    pub source_event_uid: String,
    #[serde(default)]
    pub source_task_uid: Option<String>,
    /// User-local conversation message for a manual Documentation Refresh.
    /// The message body stays outside the shared planning repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_message_id: Option<String>,
    pub source_revision: u64,
    pub trigger: SpecSyncTriggerKindV1,
    pub status: SpecSyncQueueStatusV1,
    pub decline_reason: Option<String>,
    pub attempts: u16,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl SpecSyncQueueEntryV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.uid)
            || !is_uuid(&self.feature_uid)
            || !is_uuid(&self.source_event_uid)
            || self.uid != self.source_event_uid
            || self
                .source_task_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self.request_message_id.as_deref().is_some_and(|id| {
                id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
            || (self.request_message_id.is_some()
                && self.trigger != SpecSyncTriggerKindV1::ManualRefresh)
            || self.revision == 0
            || self.source_revision == 0
            || self.attempts > 1000
            || self.created_at_ms == 0
            || self.updated_at_ms < self.created_at_ms
            || self
                .decline_reason
                .as_deref()
                .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 1024)
            || (self.status == SpecSyncQueueStatusV1::Declined) != self.decline_reason.is_some()
        {
            return Err("specification sync queue entry is invalid".into());
        }
        Ok(())
    }
}

fn has_duplicates(values: &[String]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}
