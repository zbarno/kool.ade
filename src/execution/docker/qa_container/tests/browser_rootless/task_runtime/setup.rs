use sha2::Digest as _;
use std::{fs, path::PathBuf};

use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ProjectRuntimeStore, ReadinessProbeV1, RecipeSource,
        TargetCapabilityState,
    },
    ui::layout::RuntimeTargetApprovalProof,
};

#[path = "setup/git.rs"]
mod git;
use git::{copy_app_fixture, git, git_commit, git_output, git_text, initialize_git};

pub(super) struct Fixture {
    pub projects_root: PathBuf,
    pub task_clone: PathBuf,
    pub planning: PlanningStore,
    pub workspace: WorkspaceRef,
    pub target_id: String,
    pub complete_diff_hash: String,
    _approval_cleanup: crate::persistence::runtime_target_approvals::WorkspaceCleanup,
    _home: crate::test_support::KooladeHomeGuard,
    pub root: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary runtime QA fixture");
        let home_root = root.path().join("operator-home");
        fs::create_dir_all(&home_root).unwrap();
        let home = crate::test_support::KooladeHomeGuard::set(&home_root);
        let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let projects_root = root.path().join("projects");
        let task_clone = projects_root
            .join(&project_id)
            .join("task-repositories")
            .join("fixture")
            .join(&task_id);
        fs::create_dir_all(&task_clone).unwrap();
        initialize_git(&task_clone);
        fs::write(task_clone.join("README.md"), "Synthetic task baseline.\n").unwrap();
        git(&task_clone, &["add", "README.md"]);
        git_commit(&task_clone, "Create task baseline");
        let base_sha = git_text(&task_clone, &["rev-parse", "HEAD"]);
        copy_app_fixture(&task_clone);
        git(&task_clone, &["add", "--all"]);
        git_commit(&task_clone, "Add Python task web application");
        let source_sha = git_text(&task_clone, &["rev-parse", "HEAD"]);
        let diff = git_output(
            &task_clone,
            &["diff", "--binary", &format!("{base_sha}..{source_sha}")],
        );
        let complete_diff_hash = format!("{:x}", sha2::Sha256::digest(diff));
        let workspace = WorkspaceRef {
            project_id: project_id.clone(),
            task_id: task_id.clone(),
            source_sha: source_sha.clone(),
        };
        let approval_cleanup =
            crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&project_id);
        let planning = PlanningStore::legacy_embedded(
            uuid::Uuid::parse_str(&project_id).unwrap(),
            &root.path().join("project"),
        );
        let runtime_store = ProjectRuntimeStore::new(&planning);
        let mut manifest = ProjectCapabilityManifestV1 {
            schema_version: 1,
            workspace_id: project_id,
            repository_id: "synthetic-runtime-qa".into(),
            source_sha,
            detected_languages: vec!["python".into()],
            frameworks: Vec::new(),
            candidate_targets: Vec::new(),
            selected_target_id: None,
            target_capabilities: Vec::new(),
            recipes: Vec::new(),
            readiness_probes: Default::default(),
            artifact_specs: Default::default(),
            confidence: 100,
            evidence_refs: Vec::new(),
            user_overrides: Vec::new(),
            revision: 0,
        };
        runtime_store.save_manifest(&manifest, 0).unwrap();
        manifest.revision = 1;
        let proof = RuntimeTargetApprovalProof::for_test();
        let target_id = format!("user-target-{}", uuid::Uuid::new_v4().simple());
        let readiness = ReadinessProbeV1::HttpOnApprovedPort {
            port: 4173,
            path: "/".into(),
            expected_status: 200,
        };
        let artifact_paths = vec!["dist/app.py".into(), "dist/index.html".into()];
        let recipes = recipes(&proof.receipt_uid());
        let target = AppTargetV1 {
            target_id: target_id.clone(),
            owning_task_id: task_id.clone(),
            repository_id: manifest.repository_id.clone(),
            app_surface: AppSurface::Web,
            project_relative_root: ".".into(),
            build_recipe_ref: Some("task-web-build".into()),
            test_recipe_refs: vec!["task-web-test".into()],
            start_recipe_ref: Some("task-web-start".into()),
            readiness_probe_ref: None,
            artifact_spec_ref: Some("task-web-artifacts".into()),
            fixture_ref: Some("python-task-web-v1".into()),
            required_dependencies: Vec::new(),
            supported_driver_ids: vec!["browser".into()],
            port_needs: Vec::new(),
            environment_policy_ref: "qa-approved-port-only".into(),
            capability_state: TargetCapabilityState::ReadyButUntested,
        };
        let proposal_hash = ProjectRuntimeStore::user_target_proposal_sha256(
            &manifest.workspace_id,
            &manifest.repository_id,
            &manifest.source_sha,
            &target,
            &recipes,
            Some(&readiness),
            Some(&artifact_paths),
        )
        .unwrap();
        let receipt = GateReceipt {
            receipt_uid: proof.receipt_uid(),
            subject_uid: task_id.clone(),
            subject_sha256: proposal_hash,
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: now_ms(),
            evidence_refs: vec![format!("runtime-target:{target_id}")],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        };
        let manifest = runtime_store
            .add_user_target_with_readiness_and_save(
                target,
                recipes,
                Some(readiness),
                artifact_paths,
                receipt,
                proof,
                manifest.revision,
            )
            .unwrap();
        runtime_store
            .select_target_and_save(&task_id, &target_id, manifest.revision)
            .unwrap();
        Self {
            root,
            projects_root,
            task_clone,
            planning,
            workspace,
            target_id,
            complete_diff_hash,
            _approval_cleanup: approval_cleanup,
            _home: home,
        }
    }
}

fn recipes(approval_id: &str) -> Vec<ExecutionRecipeV1> {
    let evidence_refs = vec![
        format!("user-approval:{approval_id}"),
        format!("user-recipe-approval:{approval_id}"),
    ];
    vec![
        recipe(
            "task-web-build",
            vec!["-B", "build.py"],
            vec!["dist"],
            vec!["dist/app.py", "dist/index.html"],
            NetworkRequirement::Denied,
            evidence_refs.clone(),
        ),
        recipe(
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

fn recipe(
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
