use super::super::{Work, WorkKind, WorkStatus};
use crate::artifacts::planning_store::{PlanningStore, StoreError, paths};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWork {
    key: String,
    title: String,
    request: String,
    column: usize,
    feature: Option<String>,
    detail: String,
}

pub(super) fn legacy_work_exists(store: &PlanningStore) -> anyhow::Result<bool> {
    match store.read(paths::WORK) {
        Ok(_) => Ok(true),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub(super) fn read_legacy_work(store: &PlanningStore) -> anyhow::Result<(Vec<Work>, bool)> {
    let bytes = match store.read(paths::WORK) {
        Ok(bytes) => bytes,
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), false));
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let work = if value.is_array() {
        serde_json::from_value::<Vec<LegacyWork>>(value)?
            .into_iter()
            .map(migrate_legacy)
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        let file: super::super::WorkFile = serde_json::from_value(value)?;
        anyhow::ensure!(
            (1..=super::super::SCHEMA_VERSION).contains(&file.schema_version),
            "Unsupported planning work schema version {}",
            file.schema_version
        );
        file.items
    };
    let mut work = work;
    super::super::normalize_task_intents(&mut work);
    super::validate(&work)?;
    Ok((work, true))
}

fn migrate_legacy(work: LegacyWork) -> anyhow::Result<Work> {
    Ok(Work {
        uid: legacy_uid(&work.key),
        key: work.key,
        kind: WorkKind::Feature,
        title: work.title,
        original_goal: Some(work.request.clone()),
        request: work.request,
        status: WorkStatus::from_legacy_column(work.column)?,
        feature_id: work.feature,
        feature_uid: None,
        parent_uid: None,
        task_intent_uid: None,
        task_intent_history: Vec::new(),
        task_intent_cycles: Vec::new(),
        task_intent_cycle_uid: None,
        task_intent_archived_at_ms: None,
        source_branch: None,
        destination_branch: None,
        routing_overrides: std::collections::BTreeMap::new(),
        routing_inherited_from: None,
        follow_up_task: None,
        code_review_disposition: None,
        detail: work.detail,
        record_revision: 0,
        record_baseline: None,
    })
}

fn legacy_uid(key: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut bytes: [u8; 16] = Sha256::digest(key.as_bytes())[..16]
        .try_into()
        .expect("SHA-256 provides at least 16 bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}
