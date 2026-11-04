use crate::artifacts::planning_store::StoreError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestPreparationStatus {
    AwaitingAuthorization,
    Publishing,
    NeedsAttention,
    Opened,
    ClosedUnmerged,
    Merged,
}

/// Transport recovery belongs to PR work, never to the completed implementation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrPublicationRecoveryV1 {
    pub attempt: u64,
    /// The last blocker is retained through retries for audit and crash recovery.
    pub last_failure: Option<String>,
}

mod snapshot;
pub use snapshot::{PrPublicationScopeV1, PrPublicationTaskEvidenceV1};

/// Durable, idempotently identified follow-up work created with task Done.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestPreparation {
    pub schema_version: u16,
    /// Canonical planning-store record identity.
    pub uid: String,
    pub work_item_uid: String,
    pub feature_uid: String,
    pub task_uid: String,
    /// The parent task and its directly owned subtasks are scoped into this PR.
    pub included_task_uids: Vec<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub status: PullRequestPreparationStatus,
    /// Publication authority must be granted through a separate app-side action.
    pub publication_authorization: Option<crate::core::lifecycle::GateReceipt>,
    /// Frozen application-validated scope bound to the user's publication action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_scope: Option<PrPublicationScopeV1>,
    /// Fences stale publisher completions after a later retry takes ownership.
    #[serde(default)]
    pub lease_epoch: u64,
    /// Exact lifecycle events that authorized a publication attempt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publication_authorization_event_uids: Vec<String>,
    /// Absent on records created before durable publication recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<PrPublicationRecoveryV1>,
    #[serde(default)]
    pub provider_pr_id: Option<u64>,
    #[serde(default)]
    pub provider_pr_url: Option<String>,
    #[serde(default)]
    pub head_sha: Option<String>,
    #[serde(default)]
    pub base_sha: Option<String>,
    /// Merge commit SHA reported by the provider; it does not prove local checkout state.
    #[serde(default)]
    pub merged_commit_sha: Option<String>,
}

impl PullRequestPreparation {
    /// Effective epoch also reads older recovery records created before the
    /// explicit fencing field was introduced.
    pub fn publication_lease_epoch(&self) -> u64 {
        self.lease_epoch.max(self.recovery.as_ref().map_or(
            u64::from(self.publication_authorization.is_some()),
            |recovery| recovery.attempt,
        ))
    }
}

/// Provider state observed after a pull request publication or refresh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequestPublicationState {
    pub status: PullRequestPreparationStatus,
    pub provider_pr_id: Option<u64>,
    pub provider_pr_url: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub merged_commit_sha: Option<String>,
    /// Required when completing an in-flight publication attempt.
    pub lease_epoch: Option<u64>,
}

impl PullRequestPreparation {
    pub(in crate::core::lifecycle::store) fn validate(&self) -> Result<(), StoreError> {
        let valid_uid = |value: &str| uuid::Uuid::parse_str(value).is_ok();
        if self.schema_version != 1
            || !valid_uid(&self.uid)
            || !valid_uid(&self.work_item_uid)
            || self.uid != self.work_item_uid
            || !valid_uid(&self.feature_uid)
            || !valid_uid(&self.task_uid)
            || self.included_task_uids.is_empty()
            || self.included_task_uids.len() > 256
            || self.publication_authorization_event_uids.len() > 64
            || self
                .publication_authorization_event_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| {
                    !valid_uid(uid)
                        || self.publication_authorization_event_uids[..index]
                            .iter()
                            .any(|prior| prior == uid)
                })
            || self.included_task_uids.iter().any(|uid| !valid_uid(uid))
            || !self.included_task_uids.contains(&self.task_uid)
            || self
                .included_task_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| {
                    self.included_task_uids[..index]
                        .iter()
                        .any(|prior| prior == uid)
                })
            || self.revision == 0
            || self.created_at_ms == 0
            || self.updated_at_ms < self.created_at_ms
            || self.provider_pr_id == Some(0)
            || self
                .provider_pr_url
                .as_deref()
                .is_some_and(|url| !url.starts_with("https://"))
            || self
                .head_sha
                .as_deref()
                .is_some_and(|sha| !crate::core::lifecycle::types::is_git_oid(sha))
            || self
                .base_sha
                .as_deref()
                .is_some_and(|sha| !crate::core::lifecycle::types::is_git_oid(sha))
            || self
                .merged_commit_sha
                .as_deref()
                .is_some_and(|sha| !crate::core::lifecycle::types::is_git_oid(sha))
            || self
                .publication_authorization
                .as_ref()
                .is_some_and(|receipt| {
                    receipt.validate().is_err()
                        || receipt.actor != crate::core::lifecycle::GateActor::User
                        || receipt.decision != crate::core::lifecycle::GateDecision::Approved
                        || receipt.subject_uid != self.work_item_uid
                })
            || self.publication_scope.as_ref().is_some_and(|scope| {
                scope.validate(&self.included_task_uids).is_err()
                    || scope.publication_generation == 0
            })
            || (self.publication_scope.is_some() && self.publication_authorization.is_none())
            || self.lease_epoch
                > self
                    .recovery
                    .as_ref()
                    .map_or(0, |recovery| recovery.attempt)
            || self.recovery.as_ref().is_some_and(|recovery| {
                recovery.last_failure.as_ref().is_some_and(|reason| {
                    reason.trim().is_empty() || reason.len() > 4096 || reason.contains('\0')
                }) || (self.publication_authorization.is_some() && recovery.attempt == 0)
            })
            || (!matches!(
                self.status,
                PullRequestPreparationStatus::AwaitingAuthorization
                    | PullRequestPreparationStatus::NeedsAttention
            ) && self.publication_authorization.is_none())
            || (matches!(
                self.status,
                PullRequestPreparationStatus::Opened
                    | PullRequestPreparationStatus::ClosedUnmerged
                    | PullRequestPreparationStatus::Merged
            ) && (self.provider_pr_url.is_none() || self.head_sha.is_none()))
        {
            return Err(StoreError::MalformedState(
                "pull request preparation record is invalid".into(),
            ));
        }
        Ok(())
    }
}
