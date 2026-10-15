use crate::project_runtime::{
    AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, RuntimeOperation,
};

pub(super) fn artifact_capture_paths(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    recipe: &ExecutionRecipeV1,
    operation: RuntimeOperation,
) -> anyhow::Result<Vec<String>> {
    let expected = recipe
        .expected_artifacts
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let capture = if operation == RuntimeOperation::Build {
        match target.artifact_spec_ref.as_ref() {
            Some(reference) => manifest
                .artifact_specs
                .get(reference)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("runtime artifact specification is missing"))?,
            None => recipe.expected_artifacts.clone(),
        }
    } else {
        recipe.expected_artifacts.clone()
    };
    anyhow::ensure!(
        capture.len() <= 128,
        "runtime artifact list exceeds its limit"
    );
    let capture_set = capture
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        capture_set.len() == capture.len() && expected.is_subset(&capture_set),
        "approved artifact specification conflicts with recipe outputs"
    );
    for path in &capture {
        let allowed = recipe
            .allowed_output_paths
            .iter()
            .chain(recipe.expected_artifacts.iter())
            .any(|root| {
                path == root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            });
        anyhow::ensure!(allowed, "artifact path exceeds the recipe output allowlist");
    }
    Ok(capture)
}
