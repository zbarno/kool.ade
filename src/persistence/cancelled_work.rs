//! Durable cancellation tombstones keyed by stable planning or task identity.
use std::collections::BTreeSet;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    schema_version: u32,
    ids: BTreeSet<String>,
}

pub fn load(store: &PlanningStore) -> anyhow::Result<BTreeSet<String>> {
    let bytes = match store.read(crate::artifacts::planning_store::paths::CANCELLED_WORK) {
        Ok(bytes) => bytes,
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeSet::new());
        }
        Err(error) => return Err(error.into()),
    };
    let store: Store = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        store.schema_version == 1,
        "Unsupported cancelled-work schema"
    );
    Ok(store.ids)
}

pub fn save_expected(
    store: &PlanningStore,
    ids: &BTreeSet<String>,
    expected_revision: &str,
) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec_pretty(&Store {
        schema_version: 1,
        ids: ids.clone(),
    })?;
    let (_, revision) = store.transaction_with_revision(
        &[(
            crate::artifacts::planning_store::paths::CANCELLED_WORK.to_owned(),
            bytes,
        )],
        Some(expected_revision),
    )?;
    Ok(revision)
}

pub fn planning_id(uid: &str) -> String {
    format!("planning:{uid}")
}

pub fn task_id(doc: &crate::artifacts::task_docs::TaskDocument) -> String {
    doc.identity
        .as_ref()
        .map(|identity| format!("task:{}", identity.uid))
        .unwrap_or_else(|| format!("task-path:{}", doc.path))
}
