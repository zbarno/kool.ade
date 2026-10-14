use super::{QuestionV1, questions::render_markdown};
use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};
use serde::{Deserialize, Serialize};

mod readiness;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionRecord {
    #[serde(default)]
    record_kind: Option<String>,
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    data: QuestionV1,
}

pub struct QuestionStore<'a> {
    store: &'a PlanningStore,
}

impl<'a> QuestionStore<'a> {
    pub fn new(store: &'a PlanningStore) -> Self {
        Self { store }
    }

    pub fn load(&self, uid: &str) -> Result<(QuestionV1, u64), StoreError> {
        let uid = uuid::Uuid::parse_str(uid)
            .map_err(|_| StoreError::InvalidPath(uid.to_owned()))?
            .hyphenated()
            .to_string();
        let canonical_path = question_state_path(&uid)?;
        let legacy_path = legacy_question_state_path(&uid)?;
        let record = match read_question_record(self.store, &canonical_path)? {
            Some(record) => Some(record),
            None => read_question_record(self.store, &legacy_path)?,
        }
        .ok_or_else(|| StoreError::MalformedState(format!("question {uid} does not exist")))?;
        Ok((record.data, record.revision))
    }

    pub fn list(&self) -> Result<Vec<QuestionV1>, StoreError> {
        let mut records = std::collections::BTreeMap::new();
        for collection in ["state/questions", "state/items"] {
            for path in self.store.list_record_paths(collection)? {
                let Some(uid) = path
                    .rsplit('/')
                    .next()
                    .and_then(|name| name.strip_suffix(".json"))
                else {
                    continue;
                };
                if collection == "state/items" {
                    let bytes = self.store.read(&path)?;
                    let value: serde_json::Value = serde_json::from_slice(&bytes)
                        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
                    if value.get("recordKind").and_then(serde_json::Value::as_str)
                        != Some("clarification_question_v1")
                    {
                        continue;
                    }
                }
                let record = read_question_record(self.store, &path)?.ok_or_else(|| {
                    StoreError::MalformedState(format!("question record disappeared: {path}"))
                })?;
                if record.uid != uid {
                    return Err(StoreError::MalformedState(format!(
                        "question record identity mismatch: {path}"
                    )));
                }
                // Canonical state/items records take precedence over the old
                // state/questions collection during copy-on-write migration.
                records.insert(uid.to_owned(), record.data);
            }
        }
        Ok(records.into_values().collect())
    }

    pub fn save(
        &self,
        question: &QuestionV1,
        expected_revision: u64,
    ) -> Result<String, StoreError> {
        question.validate().map_err(StoreError::MalformedState)?;
        let state_path = question_state_path(&question.uid)?;
        let content_path = format!("planning/items/{}.md", question.uid);
        let markdown = render_markdown(question).map_err(StoreError::MalformedState)?;
        let now = now_unix_ms();
        let canonical = read_question_record(self.store, &state_path)?;
        let legacy_path = legacy_question_state_path(&question.uid)?;
        let legacy = if canonical.is_none() {
            read_question_record(self.store, &legacy_path)?
        } else {
            None
        };
        let previous = canonical.as_ref().or(legacy.as_ref());
        let (created_at_ms, record_revision) = match previous {
            None => {
                if question.revision != 0 {
                    return Err(StoreError::MalformedState(
                        "new questions must start at data revision zero".into(),
                    ));
                }
                (now, 1)
            }
            Some(previous) => {
                if previous.revision != expected_revision
                    || previous.uid != question.uid
                    || question.revision != previous.data.revision.saturating_add(1)
                {
                    return Err(StoreError::StaleRecordRevision {
                        path: state_path,
                        expected: expected_revision,
                        actual: previous.revision,
                    });
                }
                // A migrated canonical path is a new record even when it is
                // copied from a legacy path with an existing outer revision.
                (
                    previous.created_at_ms,
                    if canonical.is_some() {
                        expected_revision.saturating_add(1)
                    } else {
                        1
                    },
                )
            }
        };
        let record = QuestionRecord {
            record_kind: Some("clarification_question_v1".into()),
            schema_version: 1,
            uid: question.uid.clone(),
            created_at_ms,
            updated_at_ms: now,
            revision: record_revision,
            data: question.clone(),
        };
        let json = serde_json::to_vec_pretty(&record)
            .map_err(|e| StoreError::MalformedState(e.to_string()))?;
        let changes = vec![(state_path.clone(), json), (content_path, markdown)];
        let checks = vec![RecordRevisionCheck {
            path: state_path.clone(),
            expected_revision: if canonical.is_some() {
                expected_revision
            } else {
                0
            },
        }];
        let (_, revision) = self
            .store
            .transaction_with_record_revisions(&changes, &checks)?;
        Ok(revision)
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn question_state_path(uid: &str) -> Result<String, StoreError> {
    let uid = uuid::Uuid::parse_str(uid).map_err(|_| StoreError::InvalidPath(uid.into()))?;
    Ok(format!("state/items/{}.json", uid.hyphenated()))
}

fn legacy_question_state_path(uid: &str) -> Result<String, StoreError> {
    let uid = uuid::Uuid::parse_str(uid).map_err(|_| StoreError::InvalidPath(uid.into()))?;
    Ok(format!("state/questions/{}.json", uid.hyphenated()))
}

fn read_question_record(
    store: &PlanningStore,
    path: &str,
) -> Result<Option<QuestionRecord>, StoreError> {
    match store.read_record::<QuestionRecord>(path) {
        Ok((record, revision)) => {
            if record.uid != record.data.uid
                || record.revision != revision
                || record.created_at_ms == 0
                || record.updated_at_ms < record.created_at_ms
                || record.schema_version != 1
                || record
                    .record_kind
                    .as_deref()
                    .is_some_and(|kind| kind != "clarification_question_v1")
            {
                return Err(StoreError::MalformedState(format!(
                    "question record identity or revision mismatch: {path}"
                )));
            }
            record.data.validate().map_err(StoreError::MalformedState)?;
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
