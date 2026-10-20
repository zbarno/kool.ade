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

/// Durable, idempotently identified follow-up work created with task Done.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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

// Schema v1 records written before PR publication recovery also contained
// `baseBranch` and `mergedAt`. Those values now live in the implementation's
// provider details, but old snapshots must remain readable after upgrade.
impl<'de> Deserialize<'de> for PullRequestPreparation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct CompatiblePullRequestPreparation {
            schema_version: u16,
            uid: String,
            work_item_uid: String,
            feature_uid: String,
            task_uid: String,
            included_task_uids: Vec<String>,
            revision: u64,
            created_at_ms: u64,
            updated_at_ms: u64,
            status: PullRequestPreparationStatus,
            publication_authorization: Option<crate::core::lifecycle::GateReceipt>,
            #[serde(default)]
            recovery: Option<PrPublicationRecoveryV1>,
            #[serde(default)]
            provider_pr_id: Option<u64>,
            #[serde(default)]
            provider_pr_url: Option<String>,
            #[serde(default)]
            head_sha: Option<String>,
            #[serde(default)]
            base_sha: Option<String>,
            #[serde(default)]
            merged_commit_sha: Option<String>,
            // Accepted for compatibility with the original schema-v1 format.
            #[serde(default, rename = "baseBranch")]
            _base_branch: Option<String>,
            #[serde(default, rename = "mergedAt")]
            _merged_at: Option<String>,
        }

        let CompatiblePullRequestPreparation {
            schema_version,
            uid,
            work_item_uid,
            feature_uid,
            task_uid,
            included_task_uids,
            revision,
            created_at_ms,
            updated_at_ms,
            status,
            publication_authorization,
            recovery,
            provider_pr_id,
            provider_pr_url,
            head_sha,
            base_sha,
            merged_commit_sha,
            _base_branch: _,
            _merged_at: _,
        } = <CompatiblePullRequestPreparation as serde::Deserialize>::deserialize(deserializer)?;

        Ok(Self {
            schema_version,
            uid,
            work_item_uid,
            feature_uid,
            task_uid,
            included_task_uids,
            revision,
            created_at_ms,
            updated_at_ms,
            status,
            publication_authorization,
            recovery,
            provider_pr_id,
            provider_pr_url,
            head_sha,
            base_sha,
            merged_commit_sha,
        })
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::PullRequestPreparation;

    #[test]
    fn schema_v1_records_accept_legacy_pr_details_without_emitting_them_again() {
        let legacy_record = serde_json::json!({
            "schemaVersion": 1,
            "uid": "8e25d881-f356-4e8f-8bf5-a9fa89320d33",
            "workItemUid": "8e25d881-f356-4e8f-8bf5-a9fa89320d33",
            "featureUid": "31138852-5683-44a0-8566-f6339ca7bb17",
            "taskUid": "1f131c78-2b72-49e2-aa44-1118dbf462bb",
            "includedTaskUids": ["1f131c78-2b72-49e2-aa44-1118dbf462bb"],
            "revision": 1,
            "createdAtMs": 10,
            "updatedAtMs": 10,
            "status": "awaiting_authorization",
            "publicationAuthorization": null,
            "providerPrId": null,
            "providerPrUrl": null,
            "headSha": null,
            "baseSha": null,
            "mergedCommitSha": null,
            "recovery": null,
            "baseBranch": null,
            "mergedAt": null
        });

        let record: PullRequestPreparation = serde_json::from_value(legacy_record.clone()).unwrap();
        let serialized = serde_json::to_value(record).unwrap();
        assert!(serialized.get("baseBranch").is_none());
        assert!(serialized.get("mergedAt").is_none());

        let mut legacy_record_with_values = legacy_record;
        legacy_record_with_values["baseBranch"] = serde_json::json!("main");
        legacy_record_with_values["mergedAt"] = serde_json::json!("2026-10-17T12:00:00Z");
        assert!(
            serde_json::from_value::<PullRequestPreparation>(legacy_record_with_values).is_ok()
        );
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
