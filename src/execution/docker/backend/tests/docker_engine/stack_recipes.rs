use std::{fs, process::Command, sync::Arc};

use crate::{
    execution::{
        contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1},
        docker::images::{ImageLock, RuntimeProfile},
        session::ExecutionSession,
    },
    project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource, TargetCapabilityState},
};

use super::support::{Fixture, backend, workspace};

#[test]
fn approved_stack_recipes_run_processes_without_claiming_qa_readiness() {
    const OPT_IN: &str = "KOOLADE_DOCKER_BACKEND_INTEGRATION";
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping rootless Docker stack recipe integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !rootless_docker_available() {
        eprintln!(
            "skipping rootless Docker stack recipe integration; Docker Engine is not rootless"
        );
        return;
    }

    let lock = ImageLock::bundled().expect("reviewed runtime image lock");
    for (profile, tool, build, test, start) in stack_recipes() {
        let fixture = Fixture::new();
        let workspace = workspace();
        let (docker, clone) = backend(&fixture, workspace.clone());
        write_stack_fixture(&clone, tool);
        let image = lock
            .select(profile, native_docker_arch())
            .expect("profile image for host architecture");
        assert!(image.tool_versions.contains_key(tool));
        let session = ExecutionSession::new(
            Arc::new(docker),
            workspace,
            ExecutionTarget::local(),
            ExecutionModeV1::Verification,
            image.digest,
            ResourceBudgetV1::default(),
        )
        .expect("bounded recipe session");
        let capability_state = TargetCapabilityState::ReadyButUntested;

        for recipe in [build, test, start] {
            recipe.validate().expect("approved recipe contract");
            let output = session
                .run_recipe(&recipe)
                .unwrap_or_else(|error| panic!("{} recipe failed: {error}", recipe.recipe_id));
            assert_eq!(output.result.exit_code, Some(0), "{}", recipe.recipe_id);
            assert!(!output.result.cancelled && !output.result.timed_out);
            // A process exit is not persistent application readiness evidence.
            assert_eq!(capability_state, TargetCapabilityState::ReadyButUntested);
        }
    }
}

fn rootless_docker_available() -> bool {
    Command::new("docker")
        .args(["info", "--format", "{{json .SecurityOptions}}"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains("name=rootless"))
}

fn native_docker_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => panic!("unsupported Docker integration-test architecture"),
    }
}

fn recipe(id: &str, tool: &str, argv: &[&str]) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: tool.into(),
        argv: argv.iter().map(|arg| (*arg).into()).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 180,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec![tool.into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![format!("user-approval:{id}")],
    }
}

fn stack_recipes() -> Vec<(
    RuntimeProfile,
    &'static str,
    ExecutionRecipeV1,
    ExecutionRecipeV1,
    ExecutionRecipeV1,
)> {
    vec![
        (
            RuntimeProfile::Rust,
            "cargo",
            recipe("rust-build", "cargo", &["build", "--offline"]),
            recipe("rust-test", "cargo", &["test", "--offline"]),
            recipe("rust-start", "cargo", &["run", "--offline", "--quiet"]),
        ),
        (
            RuntimeProfile::Node,
            "node",
            recipe("node-build", "node", &["--check", "app.js"]),
            recipe("node-test", "node", &["--test"]),
            recipe("node-start", "node", &["app.js"]),
        ),
        (
            RuntimeProfile::Python,
            "python",
            recipe(
                "python-build",
                "python",
                &["-m", "compileall", "-q", "app.py"],
            ),
            recipe("python-test", "python", &["-m", "unittest", "-q"]),
            recipe("python-start", "python", &["app.py"]),
        ),
        (
            RuntimeProfile::Go,
            "go",
            recipe("go-build", "go", &["build", "-o", "app", "."]),
            recipe("go-test", "go", &["test", "./..."]),
            recipe("go-start", "go", &["run", "."]),
        ),
    ]
}

fn write_stack_fixture(root: &std::path::Path, tool: &str) {
    fs::create_dir_all(root).expect("task clone exists");
    let files: &[(&str, &str)] = match tool {
        "cargo" => &[
            (
                "Cargo.toml",
                "[package]\nname = \"stack_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
            (
                "src/main.rs",
                "fn main() { println!(\"STARTED\"); }\n#[cfg(test)] mod tests { #[test] fn passes() { assert_eq!(2 + 2, 4); } }\n",
            ),
        ],
        "node" => &[
            ("app.js", "console.log('STARTED');\n"),
            (
                "app.test.js",
                "const test = require('node:test'); const assert = require('node:assert/strict'); test('math', () => assert.equal(2 + 2, 4));\n",
            ),
        ],
        "python" => &[
            (
                "app.py",
                "def main(): print('STARTED')\nif __name__ == '__main__': main()\n",
            ),
            (
                "test_app.py",
                "import unittest\nclass MathTest(unittest.TestCase):\n def test_math(self): self.assertEqual(2 + 2, 4)\n",
            ),
        ],
        "go" => &[
            (
                "go.mod",
                "module example.invalid/stackfixture\n\ngo 1.26.8\n",
            ),
            (
                "main.go",
                "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"STARTED\") }\n",
            ),
            (
                "main_test.go",
                "package main\nimport \"testing\"\nfunc TestReady(t *testing.T) {}\n",
            ),
        ],
        _ => panic!("unknown stack tool"),
    };
    for (relative, contents) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(path, contents).expect("stack fixture source");
    }
}
