use super::*;
use crate::core::planning::{
    QuestionPriority, QuestionStatus, ReadinessChecklistV1, ReadinessResult,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadinessRecord {
    schema_version: u64,
    uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    data: ReadinessChecklistV1,
}

impl QuestionStore<'_> {
    pub fn load_readiness(
        &self,
        feature_uid: &str,
    ) -> Result<Option<(ReadinessChecklistV1, u64)>, StoreError> {
        let uid = canonical_uid(feature_uid)?;
        let path = format!("state/readiness/{uid}.json");
        match self.store.read_record::<ReadinessRecord>(&path) {
            Ok((record, revision)) => {
                validate_readiness_record(&record, revision, &path)?;
                Ok(Some((record.data, revision)))
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn save_readiness(
        &self,
        checklist: &ReadinessChecklistV1,
        expected_revision: u64,
    ) -> Result<String, StoreError> {
        self.save_readiness_fenced(checklist, expected_revision, None)
    }

    fn save_readiness_fenced(
        &self,
        checklist: &ReadinessChecklistV1,
        expected_revision: u64,
        expected_planning_revision: Option<&str>,
    ) -> Result<String, StoreError> {
        checklist.validate().map_err(StoreError::MalformedState)?;
        let uid = canonical_uid(&checklist.feature_uid)?;
        let path = format!("state/readiness/{uid}.json");
        let now = now_unix_ms();
        let (created_at_ms, record_revision) = if expected_revision == 0 {
            if checklist.revision != 0 {
                return Err(StoreError::MalformedState(
                    "new readiness records must start at data revision zero".into(),
                ));
            }
            (now, 1)
        } else {
            let (previous, revision) = self.store.read_record::<ReadinessRecord>(&path)?;
            validate_readiness_record(&previous, revision, &path)?;
            if revision != expected_revision
                || previous.uid != uid
                || checklist.revision != previous.data.revision.saturating_add(1)
            {
                return Err(StoreError::StaleRecordRevision {
                    path,
                    expected: expected_revision,
                    actual: revision,
                });
            }
            (previous.created_at_ms, expected_revision.saturating_add(1))
        };
        let record = ReadinessRecord {
            schema_version: 1,
            uid,
            created_at_ms,
            updated_at_ms: now,
            revision: record_revision,
            data: checklist.clone(),
        };
        let json = serde_json::to_vec_pretty(&record)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        let checks = &[RecordRevisionCheck {
            path: path.clone(),
            expected_revision,
        }];
        let (_, revision) = if let Some(expected) = expected_planning_revision {
            self.store.transaction_with_revision_and_record_revisions(
                &[(path, json)],
                Some(expected),
                checks,
            )?
        } else {
            self.store
                .transaction_with_record_revisions(&[(path, json)], checks)?
        };
        Ok(revision)
    }

    /// Recompute and persist readiness after an answer, without inventing
    /// unrecorded assessment axes or acceptance evidence.
    pub fn reassess_readiness(
        &self,
        feature_uid: &str,
        plan_revision: u64,
    ) -> Result<Option<ReadinessResult>, StoreError> {
        let Some((mut checklist, record_revision)) = self.load_readiness(feature_uid)? else {
            return Ok(None);
        };
        if checklist.plan_revision != plan_revision {
            return Err(StoreError::MalformedState(
                "readiness checklist belongs to a different plan revision".into(),
            ));
        }
        let planning_revision = self.store.revision()?;
        let questions = self
            .list()?
            .into_iter()
            .filter(|question| {
                question.feature_uid == checklist.feature_uid
                    && question.plan_revision == checklist.plan_revision
            })
            .collect::<Vec<_>>();
        let mut required = questions
            .iter()
            .flat_map(|question| question.origin_requirement_ids.iter().cloned())
            .collect::<Vec<_>>();
        required.sort();
        required.dedup();
        let result = super::super::assess_readiness(&questions, &checklist, &required);
        checklist.ready = matches!(&result, ReadinessResult::Ready);
        checklist.unresolved_blocker_ids = questions
            .iter()
            .filter(|question| {
                question.priority == QuestionPriority::Blocking
                    && matches!(
                        question.status,
                        QuestionStatus::Draft | QuestionStatus::Open | QuestionStatus::Declined
                    )
                    && question.deferred.is_none()
            })
            .map(|question| question.uid.clone())
            .collect();
        checklist.revision = checklist
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("readiness revision is exhausted".into()))?;
        self.save_readiness_fenced(&checklist, record_revision, Some(&planning_revision))?;
        Ok(Some(result))
    }
}

fn canonical_uid(uid: &str) -> Result<String, StoreError> {
    let parsed = uuid::Uuid::parse_str(uid).map_err(|_| StoreError::InvalidPath(uid.into()))?;
    let canonical = parsed.hyphenated().to_string();
    if canonical != uid {
        return Err(StoreError::InvalidPath(uid.into()));
    }
    Ok(canonical)
}

fn validate_readiness_record(
    record: &ReadinessRecord,
    revision: u64,
    path: &str,
) -> Result<(), StoreError> {
    if record.schema_version != 1
        || record.uid != record.data.feature_uid
        || record.revision != revision
        || record.created_at_ms == 0
        || record.updated_at_ms < record.created_at_ms
    {
        return Err(StoreError::MalformedState(format!(
            "readiness record identity or revision mismatch: {path}"
        )));
    }
    record.data.validate().map_err(StoreError::MalformedState)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
