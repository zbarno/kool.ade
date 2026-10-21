use super::super::*;
use crate::artifacts::planning_store::PlanningStore;

struct Fixture(std::path::PathBuf, PlanningStore);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-runtime-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
        Self(root, store)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn build_test_and_start_approvals_remain_valid_when_added_incrementally() {
    let fixture = Fixture::new();
    let project = fixture.0.join("incremental");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("pyproject.toml"), "[project]\nname='sample'\n").unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    let context = DiscoveryContext {
        workspace_id: uuid::Uuid::new_v4().to_string(),
        repository_id: "repo-incremental".into(),
        owning_task_id: uuid::Uuid::new_v4().to_string(),
        source_sha: "f".repeat(40),
    };
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&context.workspace_id);
    let manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
    let recipe = |operation: &str| {
        let proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
        let receipt_id = proof.receipt_uid();
        let command = match operation {
            "build" => vec!["-m", "compileall", "."],
            "test" => vec!["-m", "unittest"],
            _ => vec!["-m", "http.server", "8090"],
        };
        (
            ExecutionRecipeV1 {
                schema_version: 1,
                recipe_id: format!("incremental-{operation}"),
                source: RecipeSource::UserApproved,
                working_directory_relative: ".".into(),
                executable_id: "python".into(),
                argv: command.into_iter().map(str::to_owned).collect(),
                allowed_env_keys: Vec::new(),
                allowed_output_paths: Vec::new(),
                timeout_seconds: 60,
                expected_exit_codes: vec![0],
                expected_artifacts: Vec::new(),
                cache_scope: None,
                prerequisite_tool_ids: vec!["python".into()],
                network_requirement: NetworkRequirement::Denied,
                evidence_refs: vec![format!("user-approval:{receipt_id}")],
            },
            proof,
        )
    };

    let (build, build_proof) = recipe("build");
    runtime
        .approve_recipe_and_save(&target_id, RuntimeOperation::Build, build, build_proof, 1)
        .unwrap();
    let (test, test_proof) = recipe("test");
    runtime
        .approve_recipe_and_save(&target_id, RuntimeOperation::Test, test, test_proof, 2)
        .unwrap();
    let (start, start_proof) = recipe("start");
    let manifest = runtime
        .approve_recipe_with_readiness_and_save(
            &target_id,
            RuntimeOperation::Start,
            start,
            Some(ReadinessProbeV1::LocalServiceHealth {
                port: 8090,
                path: "/health".into(),
            }),
            start_proof,
            3,
        )
        .unwrap();

    let target = &manifest.candidate_targets[0];
    for operation in [
        RuntimeOperation::Build,
        RuntimeOperation::Test,
        RuntimeOperation::Start,
    ] {
        runtime
            .verify_execution_provenance(&manifest, target, operation)
            .unwrap();
    }
}
