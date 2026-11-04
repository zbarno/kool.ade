use crate::artifacts::planning_store::PlanningStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CURRENT_PATH: &str = "state/spec_drafts/current.json";
const MAX_LOCAL_SPEC_DRAFT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalSpecDraftChange {
    pub document_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalSpecDraftV1 {
    pub schema_version: u32,
    pub base_published_spec_pin: Option<crate::core::published_specs::PublishedSpecPin>,
    pub changes: Vec<LocalSpecDraftChange>,
    pub proposal_sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredSpecDraftV1 {
    pub(super) schema_version: u32,
    pub(super) base_published_spec_pin: Option<crate::core::published_specs::PublishedSpecPin>,
    pub(super) changes: Vec<LocalSpecDraftChange>,
}

pub(crate) fn load_current(store: &PlanningStore) -> anyhow::Result<Option<LocalSpecDraftV1>> {
    let Some(bytes) = read_bounded(store, CURRENT_PATH)? else {
        return Ok(None);
    };
    let current: LocalSpecDraftV1 = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        current.schema_version == 1,
        "unsupported local specification proposal schema"
    );
    validate_changes(&current.changes)?;
    anyhow::ensure!(
        is_sha256(&current.proposal_sha256),
        "local specification proposal has an invalid snapshot digest"
    );
    let snapshot_path = format!("state/spec_drafts/{}.json", current.proposal_sha256);
    let snapshot_bytes = read_bounded(store, &snapshot_path)?
        .ok_or_else(|| anyhow::anyhow!("local specification proposal snapshot is missing"))?;
    let snapshot: StoredSpecDraftV1 = serde_json::from_slice(&snapshot_bytes)?;
    anyhow::ensure!(
        snapshot.schema_version == current.schema_version
            && snapshot.base_published_spec_pin == current.base_published_spec_pin
            && snapshot.changes == current.changes,
        "local specification proposal differs from its immutable snapshot"
    );
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&snapshot_bytes)) == current.proposal_sha256,
        "local specification proposal snapshot digest does not match"
    );
    Ok(Some(current))
}

fn read_bounded(store: &PlanningStore, relative: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let path = std::path::Path::new(relative);
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("local specification proposal path has no directory"))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("local specification proposal path is invalid"))?;
    let Some(entry) = store
        .list_files(directory)?
        .into_iter()
        .find(|entry| entry.name == filename)
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        entry.bytes <= MAX_LOCAL_SPEC_DRAFT_BYTES,
        "local specification proposal exceeds the size limit"
    );
    Ok(Some(store.read(relative)?))
}

pub(super) fn validate_changes(changes: &[LocalSpecDraftChange]) -> anyhow::Result<()> {
    anyhow::ensure!(!changes.is_empty(), "local specification proposal is empty");
    let mut ids = std::collections::BTreeSet::new();
    for change in changes {
        let id = &change.document_id;
        anyhow::ensure!(ids.insert(id), "Document {id} occurs more than once");
        if let Some(module_id) = id.strip_prefix("product:") {
            anyhow::ensure!(
                module_id != "index" && crate::artifacts::product_docs::valid_id(module_id),
                "Invalid product module ID"
            );
            crate::artifacts::product_docs::validate_module(&change.content)?;
        } else if let Some(feature_id) = id.strip_prefix("feature:") {
            crate::core::specification::validate_feature(feature_id, &change.content)?;
        } else {
            anyhow::bail!("Unknown logical document ID");
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
