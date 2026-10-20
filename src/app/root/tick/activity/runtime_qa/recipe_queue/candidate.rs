use std::path::Path;

use std::collections::BTreeSet;

use crate::{
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ProjectCapabilityManifestV1, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1,
    },
};

pub(super) fn supports_recipe_surface(surface: AppSurface) -> bool {
    matches!(
        surface,
        AppSurface::Web | AppSurface::LinuxDesktop | AppSurface::HeadlessService
    )
}

pub(super) type ApprovedCandidate = (
    String,
    RuntimeOperation,
    std::path::PathBuf,
    WorkspaceRef,
    Vec<String>,
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
        recipe.source != crate::project_runtime::RecipeSource::TrustedAdapter
            && !recipe.is_explicitly_user_approved()
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
    let mut allowed_untracked_outputs = target
        .artifact_spec_ref
        .as_ref()
        .and_then(|id| manifest.artifact_specs.get(id))
        .into_iter()
        .flatten()
        .map(|path| workspace_output_path(&target.project_relative_root, path))
        .collect::<Vec<_>>();
    allowed_untracked_outputs.extend(recipes.iter().flat_map(|recipe| {
        recipe
            .allowed_output_paths
            .iter()
            .map(|path| workspace_output_path(&target.project_relative_root, path))
    }));
    allowed_untracked_outputs.sort();
    allowed_untracked_outputs.dedup();
    Ok(Some((
        target.target_id.clone(),
        operation,
        clone_path.to_owned(),
        workspace,
        required_tools,
        allowed_untracked_outputs,
    )))
}

fn workspace_output_path(target_root: &str, path: &str) -> String {
    if target_root == "." || target_root == "./" {
        path.to_owned()
    } else if path == "." {
        target_root.to_owned()
    } else {
        format!("{target_root}/{path}")
    }
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

pub(super) fn next_operation(
    target: &AppTargetV1,
    receipts: &[RuntimeReceiptV1],
    source_sha: &str,
) -> Result<Option<(RuntimeOperation, Vec<String>)>, String> {
    let Some(build_id) = target.build_recipe_ref.as_deref() else {
        return Ok(None);
    };
    if let Some(receipt) = latest_receipt(
        receipts,
        RuntimeOperation::Build,
        &target.target_id,
        source_sha,
        build_id,
    ) {
        receipt_allows_next(receipt, "build")?;
    } else {
        return Ok(Some((RuntimeOperation::Build, vec![build_id.to_owned()])));
    }

    for recipe_id in &target.test_recipe_refs {
        if let Some(receipt) = latest_receipt(
            receipts,
            RuntimeOperation::Test,
            &target.target_id,
            source_sha,
            recipe_id,
        ) {
            receipt_allows_next(receipt, "test")?;
        } else {
            return Ok(Some((
                RuntimeOperation::Test,
                target.test_recipe_refs.clone(),
            )));
        }
    }

    let Some(start_id) = target.start_recipe_ref.as_deref() else {
        return Ok(None);
    };
    if target.readiness_probe_ref.is_none() {
        return Err("approved runtime target has no readiness probe".into());
    }
    if let Some(receipt) = latest_receipt(
        receipts,
        RuntimeOperation::Start,
        &target.target_id,
        source_sha,
        start_id,
    ) {
        receipt_allows_next(receipt, "start")?;
        Ok(None)
    } else {
        Ok(Some((RuntimeOperation::Start, vec![start_id.to_owned()])))
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
