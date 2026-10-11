use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError, paths};
use crate::core::planning_work::WorkStatus;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

mod backfill;
pub(crate) use backfill::backfill_missing;
#[cfg(test)]
pub(crate) use backfill::plan_missing as plan_backfill;

const SCHEMA_VERSION: u32 = 2;
const OLDEST_SCHEMA_VERSION: u32 = 1;

mod linkage;
pub use linkage::{ExecutionPublicationStatus, PullRequestLinkage, TaskExecutionLinkage};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskState {
    pub batch_uid: String,
    pub repository_id: String,
    pub dependency_uids: Vec<String>,
    pub status: WorkStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_status: Option<String>,
    #[serde(
        default,
        alias = "source_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(
        default,
        alias = "destination_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub destination_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_branch: Option<String>,
    #[serde(default)]
    pub publication_status: ExecutionPublicationStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pull_requests: Vec<PullRequestLinkage>,
    #[serde(skip)]
    #[doc(hidden)]
    pub(crate) legacy_linkage: bool,
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
            source_branch: metadata.source_branch.clone(),
            source_sha: None,
            destination_branch: metadata.destination_branch.clone(),
            implementation_branch: None,
            publication_status: ExecutionPublicationStatus::Planned,
            pull_requests: Vec::new(),
            legacy_linkage: false,
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
    state.legacy_linkage = record.schema_version < SCHEMA_VERSION;
    Ok(Some(state))
}

pub(crate) fn update_execution_status(
    store: &PlanningStore,
    metadata: &super::TaskMetadata,
    expected_revision: u64,
    status: WorkStatus,
    execution_status: &str,
    linkage: &TaskExecutionLinkage,
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
    anyhow::ensure!(
        linkage.repository_id == metadata.repository_id,
        "Task execution repository does not match its planning metadata"
    );
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
                    source_branch: linkage.source_branch.clone(),
                    source_sha: linkage.source_sha.clone(),
                    destination_branch: linkage.destination_branch.clone(),
                    implementation_branch: linkage.implementation_branch.clone(),
                    publication_status: linkage.publication_status,
                    pull_requests: linkage.pull_requests.clone(),
                    legacy_linkage: false,
                    revision: 0,
                },
            }
        }
    };
    record.data.legacy_linkage = record.schema_version < SCHEMA_VERSION;
    if record.revision > 0
        && record.data.status == status
        && record.data.execution_status.as_deref() == Some(execution_status)
        && record.data.repository_id == linkage.repository_id
        && record.data.source_branch == linkage.source_branch
        && record.data.source_sha == linkage.source_sha
        && record.data.destination_branch == linkage.destination_branch
        && record.data.implementation_branch == linkage.implementation_branch
        && record.data.publication_status == linkage.publication_status
        && record.data.pull_requests == linkage.pull_requests
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
    record.schema_version = SCHEMA_VERSION;
    record.updated_at_ms = now_ms().max(record.created_at_ms);
    record.data.status = status;
    record.data.execution_status = Some(execution_status.to_owned());
    record.data.repository_id = linkage.repository_id.clone();
    record.data.source_branch = linkage.source_branch.clone();
    record.data.source_sha = linkage.source_sha.clone();
    record.data.destination_branch = linkage.destination_branch.clone();
    record.data.implementation_branch = linkage.implementation_branch.clone();
    record.data.publication_status = linkage.publication_status;
    record.data.pull_requests = linkage.pull_requests.clone();
    record.data.legacy_linkage = false;
    record.data.revision = record.revision;
    linkage::validate(
        &record.data.repository_id,
        record.data.source_branch.as_deref(),
        record.data.source_sha.as_deref(),
        record.data.destination_branch.as_deref(),
        record.data.implementation_branch.as_deref(),
        &record.data.pull_requests,
        &path,
    )?;
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
        (OLDEST_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&record.schema_version),
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
    linkage::validate(
        &record.data.repository_id,
        record.data.source_branch.as_deref(),
        record.data.source_sha.as_deref(),
        record.data.destination_branch.as_deref(),
        record.data.implementation_branch.as_deref(),
        &record.data.pull_requests,
        path,
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
