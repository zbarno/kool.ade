use crate::{artifacts::planning_store::StoreError, project_runtime::AppTargetV1};

pub(super) fn validate_artifact_spec(
    target: &AppTargetV1,
    paths: &[String],
) -> Result<(), StoreError> {
    let has_build = target.build_recipe_ref.is_some();
    if target.artifact_spec_ref.is_some() != has_build
        || crate::project_runtime::validate_custom_target_artifact_paths(has_build, paths).is_err()
    {
        return Err(StoreError::MalformedState(
            "custom target artifact paths require a matching artifact spec with 1 to 128 safe paths for Build".into(),
        ));
    }
    Ok(())
}
