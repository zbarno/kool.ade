//! Persist document proposals as private drafts until explicit publication.
mod model;
#[cfg(test)]
#[path = "product_documents/tests.rs"]
mod tests;

use crate::artifacts::planning_store::PlanningRoot;
pub(crate) use model::load_current;
pub use model::{LocalSpecDraftChange, LocalSpecDraftV1};
use model::{StoredSpecDraftV1, validate_changes};
use sha2::{Digest, Sha256};

pub(super) fn changes<R: PlanningRoot + ?Sized>(
    repo: &R,
    updates: &[(String, String)],
) -> anyhow::Result<Vec<(String, String)>> {
    if updates.is_empty() {
        return Ok(Vec::new());
    }
    let store = repo.planning_store();
    let current = load_current(&store)?;
    if let Some(draft) = &current {
        anyhow::ensure!(
            draft.base_published_spec_pin == store.published_spec_pin,
            "local specification proposal is based on a stale published revision; review it against the current revision"
        );
    }
    let mut changes = current
        .map(|draft| {
            draft
                .changes
                .into_iter()
                .map(|change| (change.document_id, change.content))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    for (id, content) in updates {
        anyhow::ensure!(seen.insert(id), "Document {id} is updated more than once");
        changes.insert(id.clone(), content.clone());
    }
    let changes = changes
        .into_iter()
        .map(|(document_id, content)| LocalSpecDraftChange {
            document_id,
            content,
        })
        .collect::<Vec<_>>();
    validate_changes(&changes)?;

    let snapshot = StoredSpecDraftV1 {
        schema_version: 1,
        base_published_spec_pin: store.published_spec_pin.clone(),
        changes: changes.clone(),
    };
    let snapshot_bytes = serde_json::to_vec_pretty(&snapshot)?;
    let digest = format!("{:x}", Sha256::digest(&snapshot_bytes));
    let projection = LocalSpecDraftV1 {
        schema_version: snapshot.schema_version,
        base_published_spec_pin: snapshot.base_published_spec_pin,
        changes: snapshot.changes,
        proposal_sha256: digest.clone(),
    };
    let current_bytes = serde_json::to_vec_pretty(&projection)?;
    Ok(vec![
        (
            format!("state/spec_drafts/{digest}.json"),
            String::from_utf8(snapshot_bytes)?,
        ),
        (
            "state/spec_drafts/current.json".into(),
            String::from_utf8(current_bytes)?,
        ),
    ])
}
