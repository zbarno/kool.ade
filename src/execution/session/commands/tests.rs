use super::recipe::{recipe_source_output_paths, strip_target_root, validate_artifact_outputs};
use crate::project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};

#[test]
fn target_root_prefix_requires_a_path_boundary() {
    assert_eq!(strip_target_root("apps/web", "apps/web").unwrap(), ".");
    assert_eq!(
        strip_target_root("apps/web", "apps/web/src").unwrap(),
        "src"
    );
    assert!(strip_target_root("apps/web", "apps/webish/src").is_err());
    assert!(strip_target_root("apps/web", "apps/api/src").is_err());
}

#[test]
fn artifact_selection_must_stay_inside_approved_recipe_outputs() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: "apps/web".into(),
        executable_id: "npm".into(),
        argv: vec!["run".into(), "build".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["apps/web/dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:approval-1".into()],
    };

    assert!(validate_artifact_outputs(&recipe, "apps/web", &["dist/index.html".into()],).is_ok());
    assert!(
        validate_artifact_outputs(&recipe, "apps/web", &["private/credentials.json".into()],)
            .is_err()
    );
    assert!(validate_artifact_outputs(&recipe, ".", &["dist/index.html".into()],).is_err());
}

#[test]
fn expected_artifacts_are_also_authorized_for_source_staging() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: "apps/web".into(),
        executable_id: "npm".into(),
        argv: vec!["run".into(), "build".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["apps/web/dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:approval-1".into()],
    };

    assert_eq!(
        recipe_source_output_paths(&recipe),
        ["apps/web/dist/index.html"]
    );
}
