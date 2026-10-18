use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) const RECEIPT_FILE: &str = "subtask-integration-v1.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IntegratedSubtask {
    pub(super) task_uid: String,
    pub(super) reviewed_head_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SubtaskIntegrationReceiptV1 {
    pub(super) schema_version: u16,
    pub(super) parent_task_uid: String,
    pub(super) base_sha: String,
    pub(super) integration_head_sha: String,
    pub(super) subtasks: Vec<IntegratedSubtask>,
}

impl SubtaskIntegrationReceiptV1 {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1
                && uuid::Uuid::parse_str(&self.parent_task_uid).is_ok()
                && valid_oid(&self.base_sha)
                && valid_oid(&self.integration_head_sha)
                && !self.subtasks.is_empty()
                && self.subtasks.iter().all(|child| {
                    uuid::Uuid::parse_str(&child.task_uid).is_ok()
                        && valid_oid(&child.reviewed_head_sha)
                })
                && self
                    .subtasks
                    .iter()
                    .map(|child| &child.task_uid)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == self.subtasks.len(),
            "Subtask integration receipt is malformed"
        );
        Ok(())
    }
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn read_receipt(path: &Path) -> anyhow::Result<Option<SubtaskIntegrationReceiptV1>> {
    if !path.exists() {
        return Ok(None);
    }
    let receipt: SubtaskIntegrationReceiptV1 = serde_json::from_slice(&fs::read(path)?)?;
    receipt.validate()?;
    Ok(Some(receipt))
}
