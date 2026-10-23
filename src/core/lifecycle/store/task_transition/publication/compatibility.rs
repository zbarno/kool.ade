use super::{
    PrPublicationRecoveryV1, PrPublicationScopeV1, PullRequestPreparation,
    PullRequestPreparationStatus,
};
use serde::Deserialize;

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
            publication_scope: Option<PrPublicationScopeV1>,
            #[serde(default)]
            lease_epoch: u64,
            #[serde(default)]
            publication_authorization_event_uids: Vec<String>,
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
            publication_scope,
            lease_epoch,
            publication_authorization_event_uids,
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
            publication_scope,
            lease_epoch,
            publication_authorization_event_uids,
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
mod tests {
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

        let mut legacy_record_with_values = legacy_record.clone();
        legacy_record_with_values["baseBranch"] = serde_json::json!("main");
        legacy_record_with_values["mergedAt"] = serde_json::json!("2026-10-17T12:00:00Z");
        assert!(
            serde_json::from_value::<PullRequestPreparation>(legacy_record_with_values).is_ok()
        );

        let mut current_record = legacy_record;
        current_record["publicationScope"] = serde_json::Value::Null;
        current_record["leaseEpoch"] = serde_json::json!(2);
        current_record["publicationAuthorizationEventUids"] =
            serde_json::json!(["8e25d881-f356-4e8f-8bf5-a9fa89320d33"]);
        let record: PullRequestPreparation = serde_json::from_value(current_record).unwrap();
        assert_eq!(record.lease_epoch, 2);
        assert_eq!(
            record.publication_authorization_event_uids,
            ["8e25d881-f356-4e8f-8bf5-a9fa89320d33"]
        );
    }
}
