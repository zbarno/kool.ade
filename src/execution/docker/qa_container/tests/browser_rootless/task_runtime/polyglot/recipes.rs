use crate::project_runtime::{
    ExecutionRecipeV1, NetworkRequirement, RecipeSource, RuntimeOperation,
};

pub(super) fn recipe(
    target_root: &str,
    operation: RuntimeOperation,
    approval_id: &str,
) -> ExecutionRecipeV1 {
    let (stack, executable_id, argv, expected_artifacts, network_requirement) =
        match (target_root, operation) {
            ("apps/web", RuntimeOperation::Build) => (
                "node",
                "node",
                vec!["build.mjs"],
                vec!["dist/server.ts", "dist/index.html"],
                NetworkRequirement::Denied,
            ),
            ("apps/web", RuntimeOperation::Test) => (
                "node",
                "node",
                vec!["--test", "tests/server.test.ts"],
                Vec::new(),
                NetworkRequirement::Denied,
            ),
            ("apps/web", RuntimeOperation::Start) => (
                "node",
                "node",
                vec!["dist/server.ts"],
                Vec::new(),
                NetworkRequirement::ApprovedServicePorts,
            ),
            ("services/api", RuntimeOperation::Build) => (
                "python",
                "python",
                vec!["-B", "build.py"],
                vec!["dist/app.py", "dist/index.html"],
                NetworkRequirement::Denied,
            ),
            ("services/api", RuntimeOperation::Test) => (
                "python",
                "python",
                vec!["-B", "-m", "unittest", "-q"],
                Vec::new(),
                NetworkRequirement::Denied,
            ),
            ("services/api", RuntimeOperation::Start) => (
                "python",
                "python",
                vec!["-B", "dist/app.py"],
                Vec::new(),
                NetworkRequirement::ApprovedServicePorts,
            ),
            _ => unreachable!("only the checked-in web targets are executed"),
        };
    let id = match operation {
        RuntimeOperation::Build => "build",
        RuntimeOperation::Test => "test",
        RuntimeOperation::Start => "start",
    };
    let expected_artifacts = expected_artifacts
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: format!("polyglot-{stack}-{id}"),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: executable_id.into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: if operation == RuntimeOperation::Start {
            vec!["HOST".into(), "PORT".into()]
        } else {
            Vec::new()
        },
        allowed_output_paths: if operation == RuntimeOperation::Build {
            expected_artifacts.clone()
        } else {
            Vec::new()
        },
        timeout_seconds: 120,
        expected_exit_codes: vec![0],
        expected_artifacts,
        cache_scope: None,
        prerequisite_tool_ids: vec![stack.into()],
        network_requirement,
        evidence_refs: vec![format!("user-approval:{approval_id}")],
    }
}
