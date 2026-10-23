use super::super::super::policy_error;
use crate::{execution::errors::ExecutionError, project_runtime::ExecutionRecipeV1};

pub(in crate::execution::session::commands) fn join_target_root(
    target_root: &str,
    path: &str,
) -> Result<String, ExecutionError> {
    if target_root == "." {
        Ok(path.to_owned())
    } else if path == "." {
        Ok(target_root.to_owned())
    } else {
        Ok(format!("{target_root}/{path}"))
    }
}

pub(in crate::execution::session::commands) fn strip_target_root(
    target_root: &str,
    path: &str,
) -> Result<String, ExecutionError> {
    if target_root == "." {
        return Ok(path.to_owned());
    }
    let relative = path
        .strip_prefix(target_root)
        .filter(|suffix| suffix.is_empty() || suffix.starts_with('/'))
        .ok_or_else(|| {
            policy_error("Recipe working directory is outside its approved target root")
        })?;
    let relative = relative.strip_prefix('/').unwrap_or(relative);
    if relative.is_empty() {
        Ok(".".into())
    } else {
        Ok(relative.to_owned())
    }
}

pub(in crate::execution::session::commands) fn recipe_source_output_paths(
    recipe: &ExecutionRecipeV1,
) -> Vec<String> {
    recipe
        .allowed_output_paths
        .iter()
        .chain(recipe.expected_artifacts.iter())
        .cloned()
        .collect()
}

pub(in crate::execution::session::commands) fn validate_artifact_outputs(
    recipe: &ExecutionRecipeV1,
    target_root: &str,
    artifact_paths: &[String],
) -> Result<(), ExecutionError> {
    for artifact in artifact_paths {
        let allowed = recipe
            .allowed_output_paths
            .iter()
            .chain(recipe.expected_artifacts.iter())
            .filter_map(|path| strip_target_root(target_root, path).ok())
            .any(|root| {
                root == "."
                    || artifact == &root
                    || artifact
                        .strip_prefix(&root)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            });
        if !allowed {
            return Err(policy_error(
                "Requested artifact is outside the approved recipe outputs",
            ));
        }
    }
    Ok(())
}
