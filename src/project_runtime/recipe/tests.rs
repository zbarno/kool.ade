use super::*;

#[test]
fn custom_target_root_accepts_repository_root_and_safe_relative_paths() {
    assert!(validate_custom_target_root(".").is_ok());
    assert!(validate_custom_target_root("./").is_ok());
    assert!(validate_custom_target_root("apps/site").is_ok());
    assert_eq!(normalize_custom_target_root("./").unwrap(), ".");
    assert_eq!(
        normalize_custom_target_root("./apps/site").unwrap(),
        "apps/site"
    );
}

#[test]
fn custom_target_root_rejects_absolute_and_traversal_paths() {
    assert!(validate_custom_target_root("/workspace/app").is_err());
    assert!(validate_custom_target_root("../outside").is_err());
    assert!(validate_custom_target_root("apps/../../outside").is_err());
    assert!(validate_custom_target_root("apps\\site").is_err());
    assert!(validate_custom_target_root("apps/\nsite").is_err());
}

#[test]
fn custom_artifact_paths_reject_curdir_and_duplicates() {
    assert!(validate_custom_artifact_paths(&["dist/index.html".into()]).is_ok());
    assert!(validate_custom_artifact_paths(&["./dist/index.html".into()]).is_err());
    assert!(
        validate_custom_artifact_paths(&["dist/index.html".into(), "dist/index.html".into()])
            .is_err()
    );
    assert!(validate_custom_artifact_paths(&["dist/\tindex.html".into()]).is_err());
}

#[test]
fn custom_target_artifact_paths_follow_build_recipe_presence() {
    assert!(validate_custom_target_artifact_paths(false, &[]).is_ok());
    assert!(validate_custom_target_artifact_paths(true, &[]).is_err());
    assert!(validate_custom_target_artifact_paths(true, &["dist/index.html".into()]).is_ok());
    assert!(validate_custom_target_artifact_paths(false, &["dist/index.html".into()]).is_err());
}

#[test]
fn executable_ids_accept_logical_tools_and_canonical_target_relative_paths() {
    for executable in ["cargo", "node", "dist/app", "bin/my-app"] {
        assert!(valid_executable_id(executable), "rejected {executable:?}");
    }
    assert!(is_logical_tool_id("cargo"));
    assert!(!is_logical_tool_id("dist/app"));
    assert!(is_target_relative_executable_path("dist/app"));
}

#[test]
fn executable_ids_reject_interpreters_and_unsafe_or_noncanonical_paths() {
    for executable in [
        "sh",
        "bash",
        "dist/sh",
        "../app",
        "dist/../app",
        "/workspace/dist/app",
        "dist//app",
        "./dist/app",
        "dist/.git/app",
        "dist\\app",
        "dist/app\n",
    ] {
        assert!(!valid_executable_id(executable), "accepted {executable:?}");
    }
}

#[test]
fn approved_recipes_reject_cloud_and_ssh_environment_keys_but_allow_app_keys() {
    for key in ["AWS_REGION", "SSH_AUTH_SOCK", "SSH_AGENT_PID", "9APP_MODE"] {
        assert!(
            recipe_with_environment_key(key).validate().is_err(),
            "accepted protected key {key}"
        );
    }
    assert!(recipe_with_environment_key("APP_MODE").validate().is_ok());
}

fn recipe_with_environment_key(key: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "fixture-recipe".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "cargo".into(),
        argv: vec!["test".into()],
        allowed_env_keys: vec![key.into()],
        allowed_output_paths: Vec::new(),
        timeout_seconds: 1200,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["cargo".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:fixture".into()],
    }
}
