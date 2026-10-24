//! Durable, non-secret links between remote executions and private-file grants.

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u8 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_GRANTS_PER_EXECUTION: usize = 512;
const MAX_ACTIVE_EXECUTIONS_PER_WORKER: usize = 512;

mod store;
pub(crate) use store::{bind_start_request, load_for_worker, mark_stage_complete, remove, save};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RemotePrivateFileExecutionGrantV1 {
    pub(crate) schema_version: u8,
    pub(crate) execution_id: String,
    pub(crate) project_id: String,
    pub(crate) task_id: String,
    pub(crate) worker_id: String,
    pub(crate) controller_id: String,
    pub(crate) request_sequence: u64,
    pub(crate) cancellation_generation: u64,
    pub(crate) grant_ids: Vec<String>,
    pub(crate) start_payload_sha256: String,
    pub(crate) stage_complete: bool,
    pub(crate) start_request_sequence: Option<u64>,
}

impl RemotePrivateFileExecutionGrantV1 {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported private file execution grant schema"
        );
        for id in [&self.execution_id, &self.worker_id, &self.controller_id] {
            validate_uuid(id)?;
        }
        crate::execution::contract::validate_identifier(&self.project_id)?;
        crate::execution::contract::validate_identifier(&self.task_id)?;
        anyhow::ensure!(self.request_sequence > 0 && self.cancellation_generation > 0);
        anyhow::ensure!(
            self.start_payload_sha256.len() == 71
                && self.start_payload_sha256.starts_with("sha256:")
                && self.start_payload_sha256[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "private file execution start binding is invalid"
        );
        anyhow::ensure!(
            self.start_request_sequence
                .is_none_or(|sequence| self.stage_complete && sequence > 0),
            "private file execution start sequence is invalid"
        );
        anyhow::ensure!(
            !self.grant_ids.is_empty() && self.grant_ids.len() <= MAX_GRANTS_PER_EXECUTION,
            "private file execution grant set is invalid"
        );
        let mut previous: Option<&str> = None;
        for grant_id in &self.grant_ids {
            validate_uuid(grant_id)?;
            anyhow::ensure!(
                previous.is_none_or(|value| value < grant_id.as_str()),
                "private file execution grant IDs are not sorted and unique"
            );
            previous = Some(grant_id);
        }
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() as u64 <= MAX_RECORD_BYTES,
            "private file execution grant record is oversized"
        );
        Ok(())
    }
}

fn validate_uuid(value: &str) -> anyhow::Result<()> {
    let parsed = uuid::Uuid::parse_str(value)?;
    anyhow::ensure!(parsed.hyphenated().to_string() == value);
    Ok(())
}

#[cfg(test)]
#[path = "remote_private_file_executions/tests.rs"]
mod tests;
