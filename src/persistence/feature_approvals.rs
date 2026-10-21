//! Per-operator approvals that are deliberately excluded from shared planning Git.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct ApprovalStore {
    schema_version: u32,
    approvals: BTreeMap<String, String>,
}

fn path(repo: &std::path::Path) -> anyhow::Result<PathBuf> {
    let project_id = crate::persistence::dependency_authorization::project_id(repo)?;
    Ok(crate::persistence::project_dir(&project_id).join("feature-approvals.json"))
}

pub(crate) fn contains(repo: &std::path::Path, feature: &str, contract: &str) -> bool {
    let Ok(path) = path(repo) else { return false };
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() as usize > MAX_BYTES {
        return false;
    }
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ApprovalStore>(&bytes).ok())
        .is_some_and(|store| {
            store.schema_version == 1
                && store
                    .approvals
                    .get(feature)
                    .is_some_and(|saved| saved == contract)
        })
}

pub(crate) fn contains_scoped(repo: &std::path::Path, scope: &str, digest: &str) -> bool {
    contains(repo, &format!("scoped:{scope}"), digest)
}

pub(crate) fn record(repo: &std::path::Path, feature: &str, contract: &str) -> anyhow::Result<()> {
    let path = path(repo)?;
    let parent = path.parent().expect("approval file has a parent");
    fs::create_dir_all(parent)?;
    if let Ok(meta) = fs::symlink_metadata(&path) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "approval store must be a regular file"
        );
        anyhow::ensure!(
            meta.len() as usize <= MAX_BYTES,
            "approval store exceeds the size limit"
        );
    }
    let mut store = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ApprovalStore>(&bytes).ok())
        .filter(|store| store.schema_version == 1)
        .unwrap_or_default();
    store.schema_version = 1;
    store
        .approvals
        .insert(feature.to_owned(), contract.to_owned());
    let bytes = serde_json::to_vec(&store)?;
    anyhow::ensure!(
        bytes.len() <= MAX_BYTES,
        "approval store exceeds the size limit"
    );
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}
