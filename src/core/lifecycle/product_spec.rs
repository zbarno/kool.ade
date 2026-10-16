use crate::artifacts::{
    planning_store::{PlanningStore, StoreError},
    product_docs,
};
use sha2::Digest as _;

/// Hash the exact rendered product document representation used by history.
pub(crate) fn current_product_hash(store: &PlanningStore) -> Result<String, StoreError> {
    let rendered = product_docs::render_product(store)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        .ok_or_else(|| StoreError::MalformedState("product specification is unavailable".into()))?;
    let encoded = serde_json::to_vec(&Some(rendered))
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(format!("{:x}", sha2::Sha256::digest(encoded)))
}
