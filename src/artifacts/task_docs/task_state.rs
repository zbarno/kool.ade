use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError, paths};
use crate::core::planning_work::WorkStatus;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

mod backfill;
pub(crate) use backfill::backfill_missing;
#[cfg(test)]
pub(crate) use backfill::plan_missing as plan_backfill;

const SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
#[path = "task_state/tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskStateRecord {
    schema_version: u32,
    uid: String,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    pub(crate) data: TaskState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskState {
    pub batch_uid: String,
    pub repository_id: String,
    pub dependency_uids: Vec<String>,
    pub status: WorkStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_status: Option<String>,
    #[serde(skip)]
    #[doc(hidden)]
    pub revision: u64,
}

pub(crate) fn create(
    metadata: &super::TaskMetadata,
) -> anyhow::Result<(String, Vec<u8>, RecordRevisionCheck)> {
    anyhow::ensure!(
        uuid::Uuid::parse_str(&metadata.uid).is_ok(),
        "Task state has an invalid UID"
    );
    let timestamp = now_ms();
    let record = TaskStateRecord {
        schema_version: SCHEMA_VERSION,
        uid: metadata.uid.clone(),
        revision: 1,
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        data: TaskState {
            batch_uid: metadata.batch_uid.clone(),
            repository_id: metadata.repository_id.clone(),
            dependency_uids: metadata.dependency_uids.clone(),
            status: WorkStatus::Todo,
            execution_status: None,
            revision: 1,
        },
    };
    let path = format!("{}/{}.json", paths::TASK_STATES, metadata.uid);
    Ok((
        path.clone(),
        serde_json::to_vec_pretty(&record)?,
        RecordRevisionCheck {
            path,
            expected_revision: 0,
        },
    ))
}

pub(crate) fn load(store: &PlanningStore, uid: &str) -> anyhow::Result<Option<TaskState>> {
    let uid = uuid::Uuid::parse_str(uid)?.hyphenated().to_string();
    let path = format!("{}/{uid}.json", paths::TASK_STATES);
    let record = match read_record(store, &path)? {
        Some(record) => record,
        None => return Ok(None),
    };
    validate_record(&record, &uid, &path)?;
    let mut state = record.data;
    state.revision = record.revision;
    Ok(Some(state))
}

pub(crate) fn update_execution_status(
    store: &PlanningStore,
    metadata: &super::TaskMetadata,
    expected_revision: u64,
    status: WorkStatus,
    execution_status: &str,
) -> anyhow::Result<TaskState> {
    anyhow::ensure!(
        !execution_status.is_empty()
            && execution_status.len() <= 64
            && execution_status
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "Invalid task execution status"
    );
    let uid = uuid::Uuid::parse_str(&metadata.uid)?
        .hyphenated()
        .to_string();
    let path = format!("{}/{uid}.json", paths::TASK_STATES);
    let mut record = match read_record(store, &path)? {
        Some(record) => {
            validate_record(&record, &uid, &path)?;
            record
        }
        None => {
            anyhow::ensure!(
                expected_revision == 0,
                "Task state {path} disappeared before its update"
            );
            let timestamp = now_ms();
            TaskStateRecord {
                schema_version: SCHEMA_VERSION,
                uid: uid.clone(),
                revision: 0,
                created_at_ms: timestamp,
                updated_at_ms: timestamp,
                data: TaskState {
                    batch_uid: metadata.batch_uid.clone(),
                    repository_id: metadata.repository_id.clone(),
                    dependency_uids: metadata.dependency_uids.clone(),
                    status,
                    execution_status: Some(execution_status.to_owned()),
                    revision: 0,
                },
            }
        }
    };
    if record.revision > 0
        && record.data.status == status
        && record.data.execution_status.as_deref() == Some(execution_status)
    {
        record.data.revision = record.revision;
        return Ok(record.data);
    }
    if record.revision != expected_revision {
        return Err(
            crate::artifacts::planning_store::StoreError::StaleRecordRevision {
                path,
                expected: expected_revision,
                actual: record.revision,
            }
            .into(),
        );
    }
    record.revision = expected_revision.saturating_add(1);
    record.updated_at_ms = now_ms().max(record.created_at_ms);
    record.data.status = status;
    record.data.execution_status = Some(execution_status.to_owned());
    record.data.revision = record.revision;
    let bytes = serde_json::to_vec_pretty(&record)?;
    store.transaction_with_record_revisions(
        &[(path.clone(), bytes)],
        &[RecordRevisionCheck {
            path,
            expected_revision,
        }],
    )?;
    Ok(record.data)
}

fn read_record(store: &PlanningStore, path: &str) -> anyhow::Result<Option<TaskStateRecord>> {
    match store.read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_record(record: &TaskStateRecord, uid: &str, path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        record.schema_version == SCHEMA_VERSION,
        "Unsupported task state schema in {path}"
    );
    anyhow::ensure!(record.uid == uid, "Task state UID mismatch in {path}");
    anyhow::ensure!(record.revision > 0, "Task state {path} has revision zero");
    anyhow::ensure!(
        record.created_at_ms <= record.updated_at_ms,
        "Task state {path} has invalid timestamps"
    );
    anyhow::ensure!(
        uuid::Uuid::parse_str(&record.data.batch_uid).is_ok(),
        "Task state {path} has an invalid batch UID"
    );
    anyhow::ensure!(
        record
            .data
            .dependency_uids
            .iter()
            .all(|dependency| uuid::Uuid::parse_str(dependency).is_ok()),
        "Task state {path} has an invalid dependency UID"
    );
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
