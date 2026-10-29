use std::{fs, path::Path};

use crate::project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};

pub(super) fn write_typescript_web_app(root: &Path) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/typescript_task_web");
    for relative in [
        "package.json",
        "build.mjs",
        "src/server.ts",
        "src/index.html",
        "tests/server.test.ts",
    ] {
        let source = fixture.join(relative);
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}

pub(super) fn recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = vec![
        format!("user-approval:{approval_id}"),
        format!("user-recipe-approval:{approval_id}"),
    ];
    vec![
        recipe(
            "task-typescript-web-build",
            vec!["build.mjs"],
            vec!["dist"],
            vec!["dist/server.ts", "dist/index.html"],
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        recipe(
            "task-typescript-web-test",
            vec!["--test", "tests/server.test.ts"],
            Vec::new(),
            Vec::new(),
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-typescript-web-start".into(),
            source: RecipeSource::UserApproved,
            working_directory_relative: ".".into(),
            executable_id: "node".into(),
            argv: vec!["dist/server.ts".into()],
            allowed_env_keys: vec!["HOST".into(), "PORT".into()],
            allowed_output_paths: Vec::new(),
            timeout_seconds: 90,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: vec!["node".into()],
            network_requirement: NetworkRequirement::ApprovedServicePorts,
            evidence_refs,
        },
    ]
}

fn recipe(
    recipe_id: &str,
    argv: Vec<&str>,
    output_paths: Vec<&str>,
    artifacts: Vec<&str>,
    network_requirement: NetworkRequirement,
    evidence_refs: Vec<String>,
) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: output_paths.into_iter().map(str::to_owned).collect(),
        timeout_seconds: 120,
        expected_exit_codes: vec![0],
        expected_artifacts: artifacts.into_iter().map(str::to_owned).collect(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement,
        evidence_refs,
    }
}
