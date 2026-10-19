use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::StoreError;

pub(super) mod origin;

/// Immutable evidence that a merged PR commit is present in a mapped local
/// repository checkout. Provider-reported SHA values alone never create it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrentImplementationEvidenceV1 {
    pub schema_version: u16,
    pub uid: String,
    pub feature_uid: String,
    pub task_uid: String,
    pub work_item_uid: String,
    pub repository_id: String,
    pub base_commit_sha: String,
    pub pull_request_head_sha: String,
    pub merge_commit_sha: String,
    pub checked_out_head_sha: String,
    pub specification_revision: u64,
    pub specification_hash: String,
    pub revision: u64,
    pub recorded_at_ms: u64,
}

impl CurrentImplementationEvidenceV1 {
    fn validate(&self) -> Result<(), StoreError> {
        let oid = |value: &str| {
            matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if self.schema_version != 1
            || !crate::core::lifecycle::types::is_uuid(&self.uid)
            || !crate::core::lifecycle::types::is_uuid(&self.feature_uid)
            || !crate::core::lifecycle::types::is_uuid(&self.task_uid)
            || !crate::core::lifecycle::types::is_uuid(&self.work_item_uid)
            || self.repository_id.trim().is_empty()
            || self.repository_id.len() > 80
            || !oid(&self.base_commit_sha)
            || !oid(&self.pull_request_head_sha)
            || !oid(&self.merge_commit_sha)
            || !oid(&self.checked_out_head_sha)
            || self.specification_revision == 0
            || !crate::core::lifecycle::types::is_sha256(&self.specification_hash)
            || self.revision == 0
            || self.recorded_at_ms == 0
        {
            return Err(StoreError::MalformedState(
                "current implementation evidence is malformed".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CurrentProjection {
    pub(super) schema_version: u16,
    pub(super) uid: String,
    pub(super) created_at_ms: u64,
    pub(super) updated_at_ms: u64,
    pub(super) revision: u64,
    pub(super) record: CurrentImplementationEvidenceV1,
}

mod promotion;
