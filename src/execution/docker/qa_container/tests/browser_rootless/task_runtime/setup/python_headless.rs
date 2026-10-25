use std::{fs, path::Path};

use crate::project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};

pub(super) fn write_python_headless_app(root: &Path) {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/python_headless");
    for relative in ["build.py", "src/server.py", "test_app.py"] {
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
        python_recipe(
            "task-api-build",
            vec!["-B", "build.py"],
            vec!["dist/app.py"],
            vec!["dist/app.py"],
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        python_recipe(
            "task-api-test",
            vec!["-B", "-m", "unittest", "-q"],
            Vec::new(),
            Vec::new(),
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: "task-api-start".into(),
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
