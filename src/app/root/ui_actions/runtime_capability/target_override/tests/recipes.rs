use super::super::append_recipe;
use crate::ui::task_detail::RuntimeRecipeInput;

#[test]
fn custom_target_start_declares_an_app_owned_probe_for_an_app_relative_binary() {
    let mut recipes = Vec::new();
    let input = RuntimeRecipeInput {
        executable_id: "dist/my-app".into(),
        argv_lines: String::new(),
        allowed_env_keys_lines: "APP_RUNTIME_MODE".into(),
    };
    append_recipe(
        &mut recipes,
        "start",
        crate::project_runtime::AppSurface::Web,
        &input,
        "user-approval:test",
        &["dist/my-app".into()],
    )
    .unwrap();

    assert_eq!(recipes[0].executable_id, "dist/my-app");
    assert!(recipes[0].argv.is_empty());
    assert_eq!(recipes[0].prerequisite_tool_ids, ["python"]);
    assert_eq!(
        recipes[0].allowed_env_keys,
        ["HOST", "PORT", "APP_RUNTIME_MODE"]
    );
    assert_eq!(
        recipes[0].network_requirement,
        crate::project_runtime::NetworkRequirement::ApprovedServicePorts
    );
    assert!(recipes[0].validate().is_ok());
}

#[test]
fn custom_java_service_start_keeps_app_and_probe_tools_separate() {
    let mut recipes = Vec::new();
    append_recipe(
        &mut recipes,
        "start",
        crate::project_runtime::AppSurface::HeadlessService,
        &RuntimeRecipeInput {
            executable_id: "java".into(),
            argv_lines: "-jar\ndist/service.jar".into(),
            allowed_env_keys_lines: "APP_SERVICE_MODE".into(),
        },
        "user-approval:test",
        &[],
    )
    .unwrap();

    assert_eq!(recipes[0].prerequisite_tool_ids, ["java", "python"]);
    assert_eq!(
        recipes[0].allowed_env_keys,
        ["HOST", "PORT", "APP_SERVICE_MODE"]
    );
}

#[test]
fn custom_go_start_uses_the_locked_go_runtime_for_its_health_probe() {
    let mut recipes = Vec::new();
    append_recipe(
        &mut recipes,
        "start",
        crate::project_runtime::AppSurface::HeadlessService,
        &RuntimeRecipeInput {
            executable_id: "go".into(),
            argv_lines: "run\n./cmd/api".into(),
            allowed_env_keys_lines: String::new(),
        },
        "user-approval:test",
        &[],
    )
    .unwrap();

    assert_eq!(recipes[0].prerequisite_tool_ids, ["go"]);
}

#[test]
fn custom_target_build_and_test_commands_cannot_use_artifact_paths_as_tools() {
    for operation in ["build", "test"] {
        let error = append_recipe(
            &mut Vec::new(),
            operation,
            crate::project_runtime::AppSurface::Web,
            &RuntimeRecipeInput {
                executable_id: "dist/my-app".into(),
                argv_lines: String::new(),
                allowed_env_keys_lines: String::new(),
            },
            "user-approval:test",
            &[],
        )
        .unwrap_err();
        assert!(error.contains("only supported for Start"));
    }
}

#[test]
fn custom_build_recipes_write_only_to_exact_approved_artifacts() {
    let mut recipes = Vec::new();
    append_recipe(
        &mut recipes,
        "build",
        crate::project_runtime::AppSurface::Web,
        &RuntimeRecipeInput {
            executable_id: "cargo".into(),
            argv_lines: "build\n--offline".into(),
            allowed_env_keys_lines: "APP_BUILD_MODE".into(),
        },
        "user-approval:test",
        &["target/release/app".into(), "dist/index.html".into()],
    )
    .unwrap();
    assert_eq!(
        recipes[0].allowed_output_paths,
        ["target/release/app", "dist/index.html"]
    );
    assert_eq!(recipes[0].allowed_env_keys, ["APP_BUILD_MODE"]);
}

#[test]
fn custom_test_recipes_preserve_user_selected_environment_keys() {
    let mut recipes = Vec::new();
    append_recipe(
        &mut recipes,
        "test",
        crate::project_runtime::AppSurface::Web,
        &RuntimeRecipeInput {
            executable_id: "node".into(),
            argv_lines: "test.js".into(),
            allowed_env_keys_lines: "APP_TEST_MODE".into(),
        },
        "user-approval:test",
        &[],
    )
    .unwrap();
    assert_eq!(recipes[0].allowed_env_keys, ["APP_TEST_MODE"]);
}

#[test]
fn custom_recipes_reject_protected_or_duplicate_environment_keys() {
    for (keys, expected_error) in [
        (
            "PATH",
            "environment allowlist contains an invalid or protected key",
        ),
        (
            "API_TOKEN",
            "environment allowlist contains an invalid or protected key",
        ),
        (
            "APP_MODE\nAPP_MODE",
            "environment allowlist contains a duplicate key",
        ),
    ] {
        let error = append_recipe(
            &mut Vec::new(),
            "test",
            crate::project_runtime::AppSurface::Web,
            &RuntimeRecipeInput {
                executable_id: "cargo".into(),
                argv_lines: "test".into(),
                allowed_env_keys_lines: keys.into(),
            },
            "user-approval:test",
            &[],
        )
        .unwrap_err();
        assert!(error.contains(expected_error), "{error}");
    }
}
