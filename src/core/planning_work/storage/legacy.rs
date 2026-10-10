use super::super::{Work, WorkKind, WorkStatus};
use crate::artifacts::planning_store::{PlanningStore, StoreError, paths};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub(super) fn migrate_legacy(work: super::LegacyWork) -> anyhow::Result<Work> {
    Ok(Work {
        uid: legacy_uid(&work.key),
        key: work.key,
        kind: WorkKind::Feature,
        title: work.title,
        request: work.request,
        status: WorkStatus::from_legacy_column(work.column)?,
        feature_id: work.feature,
        feature_uid: None,
        parent_uid: None,
        source_branch: None,
        destination_branch: None,
        routing_overrides: std::collections::BTreeMap::new(),
        routing_inherited_from: None,
        follow_up_task: None,
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
