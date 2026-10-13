use std::path::Path;

use std::collections::BTreeSet;

use crate::{
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppTargetV1, ProjectCapabilityManifestV1, RuntimeOperation, RuntimeReceiptState,
        RuntimeReceiptV1,
    },
};

pub(super) type ApprovedCandidate = (
    String,
    RuntimeOperation,
    std::path::PathBuf,
    WorkspaceRef,
    Vec<String>,
);

pub(super) fn approved_candidate(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    clone_path: &Path,
    workspace: WorkspaceRef,
    operation: RuntimeOperation,
    ids: Vec<&str>,
) -> Result<Option<ApprovedCandidate>, String> {
    let recipes = ids
        .iter()
        .map(|id| {
            manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == *id)
                .ok_or("approved target recipe is missing")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if recipes.iter().any(|recipe| {
        recipe.source != crate::project_runtime::RecipeSource::UserApproved
            || !recipe
                .evidence_refs
                .iter()
                .any(|reference| reference.starts_with("user-approval:"))
    }) {
        return Ok(None);
    }
    let required_tools = recipes
        .iter()
        .flat_map(|recipe| {
            std::iter::once(recipe.executable_id.as_str())
                .chain(recipe.prerequisite_tool_ids.iter().map(String::as_str))
        })
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(Some((
        target.target_id.clone(),
        operation,
        clone_path.to_owned(),
        workspace,
        required_tools,
    )))
}

pub(super) fn latest_receipt<'a>(
    receipts: &'a [RuntimeReceiptV1],
    operation: RuntimeOperation,
    target_id: &str,
    source_sha: &str,
    recipe_id: &str,
) -> Option<&'a RuntimeReceiptV1> {
    receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == operation
                && receipt.target_id == target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
}

pub(super) fn receipt_allows_next(
    receipt: &RuntimeReceiptV1,
    operation: &str,
) -> Result<(), String> {
    match receipt.state {
        RuntimeReceiptState::Passed => Ok(()),
        RuntimeReceiptState::Running => Err(format!(
            "a previous {operation} attempt did not finish; it will not be replayed automatically. Inspect its execution record and explicitly approve a fresh task revision to retry"
        )),
        RuntimeReceiptState::Failed | RuntimeReceiptState::NotApplicable => Err(format!(
            "an immutable failed {operation} receipt exists for this source SHA; revise the recipe or create a fresh task revision before retrying"
        )),
    }
}

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "../recipe_queue_tests.rs"]
mod tests;
