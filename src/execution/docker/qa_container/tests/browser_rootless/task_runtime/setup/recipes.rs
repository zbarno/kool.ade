use super::FixtureKind;
use crate::project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};

pub(super) fn recipes(approval_id: &str, kind: FixtureKind) -> Vec<ExecutionRecipeV1> {
    match kind {
        FixtureKind::PythonWeb => python_recipes(approval_id),
        FixtureKind::FalselyHealthyPythonWeb => python_recipes(approval_id),
        FixtureKind::RustWeb => rust_recipes(approval_id),
        FixtureKind::TypeScriptWeb => super::typescript_web::recipes(approval_id),
        FixtureKind::RustDesktop => rust_desktop_recipes(approval_id),
        FixtureKind::PythonHeadless => super::python_headless::recipes(approval_id),
        FixtureKind::GoHeadless => go_headless_recipes(approval_id),
    }
}

fn python_recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = evidence(approval_id);
    vec![
        python_recipe(
            "task-web-build",
            vec!["-B", "build.py"],
            vec!["dist"],
            vec!["dist/app.py", "dist/index.html"],
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        python_recipe(
            "task-web-test",
            vec!["-B", "-m", "unittest", "-q"],
            Vec::new(),
            Vec::new(),
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-web-start".into(),
            source: RecipeSource::UserApproved,
            working_directory_relative: ".".into(),
            executable_id: "python".into(),
            argv: vec!["-B".into(), "dist/app.py".into()],
            allowed_env_keys: vec!["HOST".into(), "PORT".into()],
            allowed_output_paths: Vec::new(),
            timeout_seconds: 90,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: vec!["python".into()],
            network_requirement: NetworkRequirement::ApprovedServicePorts,
            evidence_refs,
        },
    ]
}

fn rust_recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = evidence(approval_id);
    vec![
        cargo_recipe(
            "task-web-build",
            vec!["build", "--offline", "--locked", "--release"],
            evidence_refs.clone(),
        ),
        cargo_recipe(
            "task-web-test",
            vec!["test", "--offline", "--locked"],
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-web-start".into(),
            source: RecipeSource::UserApproved,
            working_directory_relative: ".".into(),
            executable_id: "target/release/runtime-qa-rust-web".into(),
            argv: Vec::new(),
            allowed_env_keys: vec!["HOST".into(), "PORT".into()],
            allowed_output_paths: Vec::new(),
            timeout_seconds: 90,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: vec!["python".into()],
            network_requirement: NetworkRequirement::ApprovedServicePorts,
            evidence_refs,
        },
    ]
}

fn rust_desktop_recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = evidence(approval_id);
    vec![
        cargo_recipe(
            "task-desktop-build",
            vec!["build", "--offline", "--locked", "--release"],
            evidence_refs.clone(),
        ),
        cargo_recipe(
            "task-desktop-test",
            vec!["test", "--offline", "--locked"],
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-desktop-start".into(),
            source: RecipeSource::UserApproved,
            working_directory_relative: ".".into(),
            executable_id: "target/release/runtime-qa-rust-desktop".into(),
            argv: Vec::new(),
            allowed_env_keys: Vec::new(),
            allowed_output_paths: Vec::new(),
            timeout_seconds: 90,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: Vec::new(),
            network_requirement: NetworkRequirement::Denied,
            evidence_refs,
        },
    ]
}

fn go_headless_recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = evidence(approval_id);
    vec![
        go_recipe(
            "task-api-build",
            vec![
                "build",
                "-trimpath",
                "-o",
                "dist/runtime-qa-go-api",
                "./cmd/api",
            ],
            vec!["dist"],
            vec!["dist/runtime-qa-go-api"],
            evidence_refs.clone(),
        ),
        go_recipe(
            "task-api-test",
            vec!["test", "-count=1", "./..."],
            Vec::new(),
            Vec::new(),
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-api-start".into(),
            source: RecipeSource::UserApproved,
            working_directory_relative: ".".into(),
            executable_id: "dist/runtime-qa-go-api".into(),
            argv: Vec::new(),
            allowed_env_keys: vec!["HOST".into(), "PORT".into()],
            allowed_output_paths: Vec::new(),
            timeout_seconds: 90,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: vec!["go".into()],
            network_requirement: NetworkRequirement::ApprovedServicePorts,
            evidence_refs,
        },
    ]
}

fn evidence(approval_id: &str) -> Vec<String> {
    vec![
        format!("user-approval:{approval_id}"),
        format!("user-recipe-approval:{approval_id}"),
    ]
}

fn python_recipe(
    recipe_id: &str,
    argv: Vec<&str>,
    allowed_output_paths: Vec<&str>,
    expected_artifacts: Vec<&str>,
    network_requirement: NetworkRequirement,
    evidence_refs: Vec<String>,
) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: allowed_output_paths
            .into_iter()
            .map(str::to_owned)
            .collect(),
        timeout_seconds: 120,
        expected_exit_codes: vec![0],
        expected_artifacts: expected_artifacts.into_iter().map(str::to_owned).collect(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement,
        evidence_refs,
    }
}

fn cargo_recipe(recipe_id: &str, args: Vec<&str>, evidence_refs: Vec<String>) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "cargo".into(),
        argv: args.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["target".into()],
        timeout_seconds: 1200,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["cargo".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs,
    }
}

fn go_recipe(
    recipe_id: &str,
    args: Vec<&str>,
    allowed_output_paths: Vec<&str>,
    expected_artifacts: Vec<&str>,
    evidence_refs: Vec<String>,
) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "go".into(),
        argv: args.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: allowed_output_paths
            .into_iter()
            .map(str::to_owned)
            .collect(),
        timeout_seconds: 1200,
        expected_exit_codes: vec![0],
        expected_artifacts: expected_artifacts.into_iter().map(str::to_owned).collect(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["go".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs,
    }
}
