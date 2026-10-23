use crate::project_runtime::{
    AppTargetV1, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
};

pub(super) fn require_passed_test_receipts(
    target: &AppTargetV1,
    receipts: &[RuntimeReceiptV1],
    source_sha: &str,
) -> anyhow::Result<()> {
    for recipe_id in &target.test_recipe_refs {
        let latest = receipts
            .iter()
            .filter(|receipt| {
                receipt.operation == RuntimeOperation::Test
                    && receipt.task_id == target.owning_task_id
                    && receipt.target_id == target.target_id
                    && receipt.source_sha == source_sha
                    && receipt.recipe_id.as_deref() == Some(recipe_id.as_str())
            })
            .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()));
        anyhow::ensure!(
            latest.is_some_and(|receipt| receipt.state == RuntimeReceiptState::Passed),
            "runtime Start requires a passing Test receipt for recipe {recipe_id} on the exact reviewed source"
        );
    }
    Ok(())
}
