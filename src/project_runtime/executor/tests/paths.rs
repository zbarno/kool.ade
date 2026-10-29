use super::super::validate_recipe_command_for_operation;
use crate::project_runtime::{
    ExecutionRecipeV1, NetworkRequirement, RecipeSource, RuntimeOperation,
};

#[test]
fn target_relative_executable_paths_are_start_only() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "start-app".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "dist/app".into(),
        argv: Vec::new(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:fixture".into()],
    };

    assert!(validate_recipe_command_for_operation(RuntimeOperation::Build, &recipe).is_err());
    assert!(validate_recipe_command_for_operation(RuntimeOperation::Test, &recipe).is_err());
    assert!(validate_recipe_command_for_operation(RuntimeOperation::Start, &recipe).is_ok());
}
