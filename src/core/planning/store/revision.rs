use serde::{Deserialize, Serialize};

use super::{
    QuestionStore, RecordChanges, RecordChangesWithChecks, legacy_question_state_path,
    question_state_path, read_question_record,
};
use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuestionCatalogRevisionV1 {
    schema_version: u16,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
}

impl QuestionStore<'_> {
    pub(crate) fn revision_check(&self, uid: &str) -> Result<RecordRevisionCheck, StoreError> {
        let uid = canonical_uuid(uid)?;
        for path in [
            question_state_path(&uid)?,
            legacy_question_state_path(&uid)?,
        ] {
            if let Some(record) = read_question_record(self.store, &path)? {
                return Ok(RecordRevisionCheck {
                    path,
                    expected_revision: record.revision,
                });
            }
        }
        Err(StoreError::MalformedState(format!(
            "question {uid} does not exist"
        )))
    }

    pub(crate) fn feature_revision_check(
        &self,
        feature_uid: &str,
    ) -> Result<RecordRevisionCheck, StoreError> {
        let feature_uid = canonical_uuid(feature_uid)?;
        let path = catalog_path(&feature_uid);
        match self.store.read_record::<QuestionCatalogRevisionV1>(&path) {
            Ok((record, actual))
                if record.schema_version == 1
                    && record.uid == feature_uid
                    && record.revision == actual
                    && record.created_at_ms > 0
                    && record.updated_at_ms >= record.created_at_ms =>
            {
                Ok(RecordRevisionCheck {
                    path,
                    expected_revision: actual,
                })
            }
            Ok(_) => Err(StoreError::MalformedState(
                "question catalog revision is inconsistent".into(),
            )),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(RecordRevisionCheck {
                    path,
                    expected_revision: 0,
                })
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) fn append_catalog_changes(
    store: &PlanningStore,
    feature_uids: &[String],
    changes: &mut RecordChanges,
    checks: &mut Vec<RecordRevisionCheck>,
) -> Result<(), StoreError> {
    let unique = feature_uids
        .iter()
        .map(|uid| canonical_uuid(uid))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    for feature_uid in unique {
        let path = catalog_path(&feature_uid);
        let (created_at_ms, expected_revision) = match store
            .read_record::<QuestionCatalogRevisionV1>(&path)
        {
            Ok((record, actual))
                if record.schema_version == 1
                    && record.uid == feature_uid
                    && record.revision == actual
                    && record.created_at_ms > 0
                    && record.updated_at_ms >= record.created_at_ms =>
            {
                (record.created_at_ms, actual)
            }
            Ok(_) => {
                return Err(StoreError::MalformedState(
                    "question catalog revision is inconsistent".into(),
                ));
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                (now_unix_ms(), 0)
            }
            Err(error) => return Err(error),
        };
        let revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("question catalog is exhausted".into()))?;
        let record = QuestionCatalogRevisionV1 {
            schema_version: 1,
            uid: feature_uid,
            created_at_ms,
            updated_at_ms: now_unix_ms(),
            revision,
        };
        changes.push((
            path.clone(),
            serde_json::to_vec_pretty(&record)
                .map_err(|error| StoreError::MalformedState(error.to_string()))?,
        ));
        checks.push(RecordRevisionCheck {
            path,
            expected_revision,
        });
    }
    Ok(())
}

pub(super) fn catalog_revision_changes(
    store: &PlanningStore,
    feature_uid: &str,
) -> Result<RecordChangesWithChecks, StoreError> {
    let mut changes = Vec::new();
    let mut checks = Vec::new();
    append_catalog_changes(
        store,
        &[canonical_uuid(feature_uid)?],
        &mut changes,
        &mut checks,
    )?;
    Ok((changes, checks))
}

fn catalog_path(feature_uid: &str) -> String {
    format!("state/question_catalog/{feature_uid}.json")
}

fn canonical_uuid(uid: &str) -> Result<String, StoreError> {
    let parsed = uuid::Uuid::parse_str(uid).map_err(|_| StoreError::InvalidPath(uid.into()))?;
    let canonical = parsed.hyphenated().to_string();
    if uid == canonical {
        Ok(canonical)
    } else {
        Err(StoreError::InvalidPath(uid.into()))
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
