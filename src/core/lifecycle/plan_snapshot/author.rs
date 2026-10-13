use super::PlanAuthorProvenanceV1;
use crate::artifacts::planning_store::{PlanningStore, StoreError};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    data: PlanAuthorProvenanceV1,
}

pub(super) fn record(
    store: &PlanningStore,
    data: &PlanAuthorProvenanceV1,
) -> Result<(), StoreError> {
    if !canonical_uuid(&data.feature_uid)
        || !canonical_uuid(&data.author_run_id)
        || !canonical_uuid(&data.author_session_id)
        || !is_hash(&data.approval_contract_sha256)
    {
        return Err(StoreError::MalformedState(
            "plan author provenance is invalid".into(),
        ));
    }
    let path = path(&data.feature_uid);
    let expected_revision = match store.read_record::<AuthorRecord>(&path) {
        Ok((record, revision)) => {
            validate_record(&record, revision, &data.feature_uid)?;
            revision
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    let now = now_ms();
    let record = AuthorRecord {
        schema_version: 1,
        uid: data.feature_uid.clone(),
        created_at_ms: now,
        updated_at_ms: now,
        revision: expected_revision + 1,
        data: data.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    store.transaction_with_record_revisions(
        &[(path.clone(), bytes)],
        &[crate::artifacts::planning_store::RecordRevisionCheck {
            path,
            expected_revision,
        }],
    )?;
    Ok(())
}

pub(super) fn load(
    store: &PlanningStore,
    feature_uid: &str,
) -> Result<Option<PlanAuthorProvenanceV1>, String> {
    match store.read_record::<AuthorRecord>(&path(feature_uid)) {
        Ok((record, revision)) => {
            validate_record(&record, revision, feature_uid).map_err(|error| error.to_string())?;
            Ok(Some(record.data))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn validate_record(
    record: &AuthorRecord,
    revision: u64,
    feature_uid: &str,
) -> Result<(), StoreError> {
    if record.schema_version != 1
        || record.uid != feature_uid
        || record.created_at_ms == 0
        || record.updated_at_ms < record.created_at_ms
        || record.revision != revision
        || record.data.feature_uid != feature_uid
        || !canonical_uuid(&record.data.author_run_id)
        || !canonical_uuid(&record.data.author_session_id)
        || !is_hash(&record.data.approval_contract_sha256)
    {
        return Err(StoreError::MalformedState(
            "plan author provenance record is invalid".into(),
        ));
    }
    Ok(())
}

fn path(feature_uid: &str) -> String {
    format!("state/lifecycle_plan_authors/{feature_uid}.json")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
