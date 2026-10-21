use crate::{
    artifacts::planning_store::StoreError,
    project_runtime::{
        AppSurface, AppTargetV1, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
    },
};

use super::ProjectRuntimeStore;

impl ProjectRuntimeStore<'_> {
    /// Return the latest passing receipts for every declared headless build and
    /// test recipe. Visual QA applicability must not bypass authorized service checks.
    pub fn headless_service_test_receipts(
        &self,
        task_id: &str,
        target: &AppTargetV1,
        source_sha: &str,
    ) -> Result<Vec<RuntimeReceiptV1>, StoreError> {
        let manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        if target.app_surface != AppSurface::HeadlessService
            || !manifest.is_active_target(task_id, &target.target_id)
            || manifest
                .candidate_targets
                .iter()
                .find(|candidate| candidate.target_id == target.target_id)
                != Some(target)
            || manifest.source_sha != source_sha
            || target.owning_task_id != task_id
        {
            return Err(StoreError::MalformedState(
                "headless service evidence does not match the active source-bound target".into(),
            ));
        }
        if target.build_recipe_ref.is_none() || target.test_recipe_refs.is_empty() {
            return Err(StoreError::MalformedState(
                "headless service applicability requires declared build and test recipes".into(),
            ));
        }
        let mut expected = Vec::new();
        if let Some(recipe_id) = &target.build_recipe_ref {
            expected.push((RuntimeOperation::Build, recipe_id.as_str()));
        }
        expected.extend(
            target
                .test_recipe_refs
                .iter()
                .map(|recipe_id| (RuntimeOperation::Test, recipe_id.as_str())),
        );
        let receipts = self.receipts_for_task(task_id)?;
        expected
            .into_iter()
            .map(|(operation, recipe_id)| {
                let receipt = receipts
                    .iter()
                    .filter(|receipt| {
                        receipt.operation == operation
                            && receipt.target_id == target.target_id
                            && receipt.source_sha == source_sha
                            && receipt.recipe_id.as_deref() == Some(recipe_id)
                    })
                    .max_by_key(|receipt| {
                        (receipt.observed_at_unix_ms, receipt.receipt_id.as_str())
                    })
                    .ok_or_else(|| {
                        StoreError::MalformedState(format!(
                            "declared headless {operation:?} recipe {recipe_id} has no passing receipt"
                        ))
                    })?;
                if receipt.state != RuntimeReceiptState::Passed {
                    return Err(StoreError::MalformedState(format!(
                        "declared headless {operation:?} recipe {recipe_id} has not passed"
                    )));
                }
                Ok(receipt.clone())
            })
            .collect()
    }
}
