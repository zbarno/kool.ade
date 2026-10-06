//! Durable cancellation tombstones keyed by stable planning or task identity.
use std::{collections::BTreeSet, fs, path::Path};

const FILE: &str = ".koolade-packet/state/cancelled-work.json";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    schema_version: u32,
    ids: BTreeSet<String>,
}

pub fn load(repo: &Path) -> anyhow::Result<BTreeSet<String>> {
    let path = repo.join(FILE);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    let store: Store = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        store.schema_version == 1,
        "Unsupported cancelled-work schema"
    );
    Ok(store.ids)
}

pub fn save(repo: &Path, ids: &BTreeSet<String>) -> anyhow::Result<()> {
    let path = repo.join(FILE);
    crate::artifacts::task_docs::safe_directory(repo, crate::artifacts::layout::canonical::STATE)?;
    let bytes = serde_json::to_vec_pretty(&Store {
        schema_version: 1,
        ids: ids.clone(),
    })?;
    crate::artifacts::atomic_write_bytes(&path, &bytes)?;
    Ok(())
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
