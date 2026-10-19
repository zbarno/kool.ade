use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::{RunRecord, SelfImprovementPhaseV1, records::validate_run_record};

const PATH: &str = "state/self_improvement_active_runs.json";
const RUNS: &str = "state/self_improvements";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveRunsRecord {
    schema_version: u16,
    revision: u64,
    run_uids: Vec<String>,
}

pub(super) struct Change {
    pub path: String,
    pub bytes: Vec<u8>,
    pub check: RecordRevisionCheck,
}

pub(super) fn reserve(
    store: &PlanningStore,
    run_uid: &str,
    limit: u8,
) -> Result<Option<Change>, StoreError> {
    let (mut record, revision) = load(store)?;
    if record.run_uids.iter().any(|uid| uid == run_uid) {
        return Ok(None);
    }
    if record.run_uids.len() >= usize::from(limit) {
        return Err(StoreError::MalformedState(
            "active self-improvement limit has been reached for this workspace".into(),
        ));
    }
    record.run_uids.push(run_uid.to_owned());
    record.run_uids.sort();
    record.revision = revision
        .checked_add(1)
        .ok_or_else(|| StoreError::MalformedState("active run revision is exhausted".into()))?;
    encode_change(record, revision).map(Some)
}

pub(super) fn release(store: &PlanningStore, run_uid: &str) -> Result<Option<Change>, StoreError> {
    let (mut record, revision) = load(store)?;
    let before = record.run_uids.len();
    record.run_uids.retain(|uid| uid != run_uid);
    if before == record.run_uids.len() {
        return Ok(None);
    }
    record.revision = revision
        .checked_add(1)
        .ok_or_else(|| StoreError::MalformedState("active run revision is exhausted".into()))?;
    encode_change(record, revision).map(Some)
}

fn load(store: &PlanningStore) -> Result<(ActiveRunsRecord, u64), StoreError> {
    match store.read_record::<ActiveRunsRecord>(PATH) {
        Ok((record, revision)) => {
            if record.schema_version != 1
                || record.revision != revision
                || record.run_uids.windows(2).any(|pair| pair[0] >= pair[1])
                || record.run_uids.iter().any(|uid| {
                    uuid::Uuid::parse_str(uid).is_err()
                        || uuid::Uuid::parse_str(uid)
                            .is_ok_and(|parsed| parsed.hyphenated().to_string() != *uid)
                })
            {
                return Err(StoreError::MalformedState(
                    "active self-improvement index is inconsistent".into(),
                ));
            }
            Ok((record, revision))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok((rebuild(store)?, 0))
        }
        Err(error) => Err(error),
    }
}

fn rebuild(store: &PlanningStore) -> Result<ActiveRunsRecord, StoreError> {
    let mut run_uids = Vec::new();
    for path in store.list_record_paths(RUNS)? {
        let (record, revision) = store.read_record::<RunRecord>(&path)?;
        validate_run_record(&record, revision, &record.uid, &path)?;
        if active(record.data.phase) {
            run_uids.push(record.uid);
        }
    }
    run_uids.sort();
    run_uids.dedup();
    Ok(ActiveRunsRecord {
        schema_version: 1,
        revision: 0,
        run_uids,
    })
}

fn encode_change(record: ActiveRunsRecord, revision: u64) -> Result<Change, StoreError> {
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(Change {
        path: PATH.into(),
        bytes,
        check: RecordRevisionCheck {
            path: PATH.into(),
            expected_revision: revision,
        },
    })
}

fn active(phase: SelfImprovementPhaseV1) -> bool {
    !matches!(
        phase,
        SelfImprovementPhaseV1::Activated | SelfImprovementPhaseV1::Cancelled
    )
}
