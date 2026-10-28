use sha2::Digest as _;
use std::fs;

use super::{
    Fixture, FixtureKind, desktop_lifecycle,
    git::{copy_app_fixture, git, git_commit, git_output, git_text, initialize_git},
    go_headless::write_go_headless_app,
    now_ms,
    recipes::recipes,
    rust_desktop::write_rust_desktop_app,
    rust_web::write_rust_web_app,
    typescript_web::write_typescript_web_app,
};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ProjectCapabilityManifestV1, ProjectRuntimeStore,
        ReadinessProbeV1, TargetCapabilityState,
    },
    ui::layout::RuntimeTargetApprovalProof,
};

pub(super) fn create_fixture(required_dependencies: Vec<String>, kind: FixtureKind) -> Fixture {
    let cache_request_ids = required_dependencies.clone();
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
    match kind {
        FixtureKind::PythonWeb => copy_app_fixture(&task_clone),
        FixtureKind::FalselyHealthyPythonWeb => super::falsely_healthy_web::write_app(&task_clone),
        FixtureKind::RustWeb => write_rust_web_app(&task_clone),
        FixtureKind::TypeScriptWeb => write_typescript_web_app(&task_clone),
        FixtureKind::RustDesktop => write_rust_desktop_app(&task_clone),
        FixtureKind::PythonHeadless => {
            super::python_headless::write_python_headless_app(&task_clone)
        }
        FixtureKind::GoHeadless => write_go_headless_app(&task_clone),
    }
    let npm_fixture = if kind == FixtureKind::TypeScriptWeb {
        Some(
            super::npm_fixture::write_lockfile_and_archive(
                &task_clone,
                &root.path().join("broker-input"),
            )
            .expect("write the lockfile-pinned TypeScript cache probe"),
        )
    } else {
        None
    };
    git(&task_clone, &["add", "--all"]);
    git_commit(
        &task_clone,
        &format!("Add {} task application", kind.language()),
    );
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
    let broker_cache = match kind {
        FixtureKind::TypeScriptWeb => npm_fixture.as_ref().map(|archive| {
            super::prepared_cache::prepare_broker_cache(&task_clone, &workspace, archive)
                .expect("prepare the lockfile through the task-bound dependency broker")
        }),
        FixtureKind::GoHeadless => Some(
            super::prepared_go_cache::prepare_broker_cache(&task_clone, &workspace)
                .expect("prepare Go modules through the task-bound dependency broker"),
        ),
        _ => None,
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
        detected_languages: vec![kind.language().into()],
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
    let readiness_path = match kind {
        FixtureKind::FalselyHealthyPythonWeb => "/health",
        FixtureKind::PythonHeadless | FixtureKind::GoHeadless => "/health",
        FixtureKind::RustDesktop => "",
        _ => "/",
    }
    .to_owned();
    let readiness = match kind.app_surface() {
        AppSurface::Web => ReadinessProbeV1::HttpOnApprovedPort {
            port: 4173,
            path: readiness_path.clone(),
            expected_status: 200,
        },
        AppSurface::HeadlessService => ReadinessProbeV1::LocalServiceHealth {
            port: 4173,
            path: "/health".into(),
        },
        AppSurface::LinuxDesktop => ReadinessProbeV1::ProcessAlive {
            executable_id: "target/release/runtime-qa-rust-desktop".into(),
        },
        _ => unreachable!("runtime fixture uses a supported app surface"),
    };
    let artifact_paths = kind.artifacts();
    let artifact_spec_ref = if kind == FixtureKind::RustDesktop {
        "task-desktop-artifacts"
    } else {
        "task-web-artifacts"
    };
    let recipes = recipes(&proof.receipt_uid(), kind);
    let (build_recipe_ref, test_recipe_id, start_recipe_ref) = match kind {
        FixtureKind::PythonWeb | FixtureKind::FalselyHealthyPythonWeb | FixtureKind::RustWeb => {
            ("task-web-build", "task-web-test", "task-web-start")
        }
        FixtureKind::TypeScriptWeb => (
            "task-typescript-web-build",
            "task-typescript-web-test",
            "task-typescript-web-start",
        ),
        FixtureKind::RustDesktop => (
            "task-desktop-build",
            "task-desktop-test",
            "task-desktop-start",
        ),
        FixtureKind::PythonHeadless | FixtureKind::GoHeadless => {
            ("task-api-build", "task-api-test", "task-api-start")
        }
    };
    let target = AppTargetV1 {
        target_id: target_id.clone(),
        owning_task_id: task_id.clone(),
        repository_id: manifest.repository_id.clone(),
        app_surface: kind.app_surface(),
        project_relative_root: ".".into(),
        build_recipe_ref: Some(build_recipe_ref.into()),
        test_recipe_refs: vec![test_recipe_id.into()],
        start_recipe_ref: Some(start_recipe_ref.into()),
        readiness_probe_ref: None,
        artifact_spec_ref: Some(artifact_spec_ref.into()),
        fixture_ref: Some(kind.fixture_ref().into()),
        required_dependencies: broker_cache
            .as_ref()
            .map(|cache| vec![cache.request_id.clone()])
            .unwrap_or(required_dependencies),
        supported_driver_ids: vec![
            match kind.app_surface() {
                AppSurface::Web => "browser",
                AppSurface::LinuxDesktop => "linux-desktop-driver",
                _ => "service",
            }
            .into(),
        ],
        // User approval binds the proposed target before persistence derives
        // runtime port requirements from its readiness configuration.
        port_needs: Vec::new(),
        environment_policy_ref: if kind.app_surface()
            == crate::project_runtime::AppSurface::LinuxDesktop
        {
            "default-deny"
        } else {
            "qa-approved-port-only"
        }
        .into(),
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
    let mut project = crate::app::session::test_support::project_from(root.path());
    project.state.planning_store = planning.clone();
    let lifecycle_task = desktop_lifecycle::reviewed_task(
        &mut project,
        &task_clone,
        &workspace,
        &complete_diff_hash,
        &base_sha,
        &manifest.repository_id,
        kind.app_surface(),
    );
    let dependency_cache = if let Some(cache) = broker_cache {
        super::prepared_cache::attach_broker_request(
            &mut project,
            &lifecycle_task,
            cache
                .request
                .as_ref()
                .expect("broker cache has request evidence"),
        );
        Some(cache)
    } else {
        super::prepared_cache::seed_synthetic(
            &mut project,
            &lifecycle_task,
            &workspace,
            &root.path().join("broker-npm-cache"),
            &cache_request_ids,
        )
    };
    Fixture {
        root,
        projects_root,
        task_clone,
        planning,
        project: Some(project),
        lifecycle_task: Some(lifecycle_task),
        workspace,
        target_id,
        build_recipe_id: build_recipe_ref.into(),
        test_recipe_id: test_recipe_id.into(),
        start_recipe_id: start_recipe_ref.into(),
        dependency_cache,
        readiness_path,
        expect_browser_failure: kind == FixtureKind::FalselyHealthyPythonWeb,
        _approval_cleanup: approval_cleanup,
        _home: home,
    }
}
