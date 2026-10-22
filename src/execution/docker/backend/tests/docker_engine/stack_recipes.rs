use std::{process::Command, sync::Arc};

use crate::{
    artifacts::planning_store::PlanningStore,
    execution::{
        contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1},
        docker::images::{ImageLock, RuntimeProfile},
        errors::ExecutionErrorKind,
        session::ExecutionSession,
    },
    project_runtime::{
        DiscoveryContext, ExecutionRecipeV1, NetworkRequirement, ProjectRuntimeStore,
        ReadinessProbeV1, RecipeSource, RuntimeOperation,
    },
};

use super::support::{Fixture, backend, workspace};

#[path = "stack_recipes/diagnostics.rs"]
mod diagnostics;
#[path = "stack_recipes/fixtures.rs"]
mod fixtures;

/// Boundary: approved Rust, Node, Python, and Go build/test/start recipes run
/// in their selected Docker profiles with the task-root and network policy,
/// and an attempt to widen the selected root is denied. Unit tests cannot
/// prove the executor images, tool resolution, workspace mounts, or command
/// isolation together. External dependency: rootless Docker and locked runtime
/// images. Setup cost: four small synthetic projects and twelve short recipes.
#[test]
fn approved_stack_recipes_execute_across_runtime_profiles() {
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
        let mut workspace = workspace();
        let _approval_cleanup = crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(
            &workspace.project_id,
        );
        let clone = fixture.task_clone(&workspace);
        fixtures::write_stack_fixture(&clone.join("apps/web"), tool);
        workspace.source_sha = Fixture::commit_task_clone(&clone);
        let (docker, _) = backend(&fixture, workspace.clone());
        let planning = PlanningStore::legacy_embedded(
            uuid::Uuid::parse_str(&workspace.project_id).unwrap(),
            &fixture.root,
        );
        let runtime_store = ProjectRuntimeStore::new(&planning);
        let context = DiscoveryContext {
            workspace_id: workspace.project_id.clone(),
            repository_id: "integration-repository".into(),
            owning_task_id: workspace.task_id.clone(),
            source_sha: workspace.source_sha.clone(),
        };
        let manifest = runtime_store
            .discover_and_save(&clone, &context, 0)
            .expect("test project capability discovery");
        let discovered_target = manifest
            .candidate_targets
            .first()
            .expect("stack fixture metadata should produce a runtime target");
        let target_id = discovered_target.target_id.clone();
        let mut manifest = runtime_store
            .select_target_and_save(&workspace.task_id, &target_id, manifest.revision)
            .expect("select integration target");
        assert_eq!(
            manifest.candidate_targets[0].project_relative_root,
            "apps/web"
        );
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
        for (operation, mut recipe) in [
            (RuntimeOperation::Build, build),
            (RuntimeOperation::Test, test),
            (RuntimeOperation::Start, start),
        ] {
            let approval_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
            let approval_id = approval_proof.receipt_uid();
            recipe
                .evidence_refs
                .retain(|reference| !reference.starts_with("user-approval:"));
            recipe
                .evidence_refs
                .push(format!("user-approval:{approval_id}"));
            manifest = if operation == RuntimeOperation::Start {
                runtime_store
                    .approve_recipe_with_readiness_and_save(
                        &target_id,
                        operation,
                        recipe.clone(),
                        Some(ReadinessProbeV1::ProcessAlive {
                            executable_id: recipe.executable_id.clone(),
                        }),
                        approval_proof,
                        manifest.revision,
                    )
                    .expect("explicitly approve start recipe and probe")
            } else {
                runtime_store
                    .approve_recipe_and_save(
                        &target_id,
                        operation,
                        recipe.clone(),
                        approval_proof,
                        manifest.revision,
                    )
                    .expect("explicitly approve stack recipe")
            };
            recipe = manifest
                .user_overrides
                .iter()
                .find(|approved| approved.recipe_id == recipe.recipe_id)
                .cloned()
                .expect("persisted approved recipe");
            recipe.validate().expect("approved recipe contract");
            let widened_root = session
                .run_scoped_recipe(&runtime_store, &target_id, &recipe, ".")
                .expect_err("a broader caller-selected root must be denied");
            assert_eq!(widened_root.kind, ExecutionErrorKind::PolicyDenied);
            let output = session
                .run_recipe(&runtime_store, &target_id, &recipe)
                .unwrap_or_else(|error| {
                    panic!(
                        "{} recipe failed: {error}; checkout: {}",
                        recipe.recipe_id,
                        diagnostics::checkout(&clone)
                    )
                });
            assert_eq!(
                output.result.exit_code,
                Some(0),
                "{} failed: stdout={:?}, stderr={:?}, result={:?}",
                recipe.recipe_id,
                output.stdout,
                output.stderr,
                output.result.failure
            );
            assert!(!output.result.cancelled && !output.result.timed_out);
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
        allowed_output_paths: if tool == "cargo" {
            vec![".koolade-target".into()]
        } else if tool == "python" {
            vec!["__pycache__".into()]
        } else if id == "go-build" {
            vec![".koolade-target".into()]
        } else {
            Vec::new()
        },
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
            recipe("rust-build", "cargo", &["build", "--offline", "--locked"]),
            recipe(
                "rust-test",
                "cargo",
                &["test", "--offline", "--locked", "-vv"],
            ),
            recipe(
                "rust-start",
                "cargo",
                &["run", "--offline", "--locked", "--quiet"],
            ),
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
            recipe(
                "go-build",
                "go",
                &["build", "-o", ".koolade-target/stack-fixture", "main.go"],
            ),
            recipe("go-test", "go", &["test", "main.go", "main_test.go"]),
            recipe("go-start", "go", &["run", "main.go"]),
        ),
    ]
}
