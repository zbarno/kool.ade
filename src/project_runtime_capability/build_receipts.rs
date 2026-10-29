use crate::project_runtime::{
    AppTargetV1, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
};

/// Projects a failed Build receipt only when it belongs to the exact selected
/// task, target, recipe, and source revision. Receipts remain immutable evidence.
pub fn observed_build_capability_state(
    target: &AppTargetV1,
    task_id: &str,
    source_sha: &str,
    receipts: &[RuntimeReceiptV1],
) -> Option<TargetCapabilityState> {
    let recipe_id = target.build_recipe_ref.as_deref()?;
    let latest = receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Build
                && receipt.task_id == task_id
                && receipt.target_id == target.target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.as_str()));
    latest
        .filter(|receipt| receipt.state == RuntimeReceiptState::Failed)
        .map(|_| TargetCapabilityState::BuildFailed)
}

#[cfg(test)]
#[path = "build_receipts_tests.rs"]
mod tests;
