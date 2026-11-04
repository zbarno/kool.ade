use crate::artifacts::{
    planning_store::{PlanningStore, StoreError},
    product_docs,
};
use sha2::Digest as _;

/// Hash the exact rendered product document representation used by history.
pub(crate) fn current_product_hash(store: &PlanningStore) -> Result<String, StoreError> {
    let updates = crate::core::apply::load_current(store)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        .map(|draft| {
            draft
                .changes
                .into_iter()
                .filter(|change| change.document_id.starts_with("product:"))
                .map(|change| (change.document_id, change.content))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let rendered = product_docs::render_product_with_updates(store, &updates)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        .ok_or_else(|| StoreError::MalformedState("product specification is unavailable".into()))?;
    let encoded = serde_json::to_vec(&Some(rendered))
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(format!("{:x}", sha2::Sha256::digest(encoded)))
}
