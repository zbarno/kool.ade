use std::sync::Arc;

use crate::{
    execution::{
        contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1},
        docker::images::{ImageLock, RuntimeProfile},
        session::ExecutionSession,
    },
    project_runtime::{
        ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation, RuntimeReceiptState,
    },
};

use super::fixture::Fixture;

pub(super) fn run(fixture: Fixture, dependency_request_id: String) {
    let docker = super::super::rootless_action_endpoint().connect().unwrap();
    let info = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(docker.info())
        .unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "polyglot runtime QA must run on rootless Docker"
    );
    let lock = super::super::candidate_lock();
    let runtime_store = ProjectRuntimeStore::new(&fixture.planning);
    let executor = ProjectRuntimeExecutor::new(&runtime_store);
    let mut manifest = runtime_store.load_manifest().unwrap().unwrap();
    let mut observed = Vec::new();

    for (target_root, profile, dependencies) in [
        (
            "apps/web",
            RuntimeProfile::Node,
            vec![dependency_request_id.clone()],
        ),
        ("services/api", RuntimeProfile::Python, Vec::new()),
    ] {
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.project_relative_root == target_root)
            .expect("expected discovered polyglot target")
            .clone();
        manifest = runtime_store
            .select_target_and_save(
                &fixture.workspace.task_id,
                &target.target_id,
                manifest.revision,
            )
            .expect("explicitly select one nested task target");
        assert!(manifest.is_active_target(&fixture.workspace.task_id, &target.target_id));

        let build_test_session = session(&fixture, &lock, profile);
        let cache = if target_root == "apps/web" {
            Some(fixture.dependency_cache.snapshot.clone())
        } else {
            None
        };
        if let Some(snapshot) = &cache {
            build_test_session
                .stage_dependency_cache_for_request(&dependency_request_id, snapshot.clone())
                .unwrap();
        }

        let build = executor
            .execute(
                &build_test_session,
                &target.target_id,
                RuntimeOperation::Build,
            )
            .unwrap()
            .remove(0);
        let test = executor
            .execute(
                &build_test_session,
                &target.target_id,
                RuntimeOperation::Test,
            )
            .unwrap()
            .remove(0);
        let start_session = session(&fixture, &lock, profile);
        if let Some(snapshot) = &cache {
            start_session
                .stage_dependency_cache_for_request(&dependency_request_id, snapshot.clone())
                .unwrap();
        }
        let start = executor
            .execute(&start_session, &target.target_id, RuntimeOperation::Start)
            .unwrap()
            .remove(0);
        start_session.stop_runtime_processes().unwrap();

        let stack = if target_root == "apps/web" {
            "node"
        } else {
            "python"
        };
        let build_artifacts = if target_root == "apps/web" {
            ["apps/web/dist/server.ts", "apps/web/dist/index.html"]
        } else {
            ["services/api/dist/app.py", "services/api/dist/index.html"]
        };
        assert!(
            build_artifacts
                .iter()
                .all(|path| build.artifact_hashes.contains_key(*path))
        );

        let dispatch: crate::app::root::test_runtime_qa::RootlessTaskQaDispatch =
            crate::app::root::test_runtime_qa::prepare_rootless_task_qa_dispatch(
                &fixture.project,
                &fixture.lifecycle_task,
            )
            .expect("production request preparation accepts the selected target receipts");
        assert_eq!(dispatch.request.code_head_sha, fixture.workspace.source_sha);
        assert_eq!(
            dispatch.request.target_id.as_deref(),
            Some(target.target_id.as_str())
        );
        assert_eq!(
            dispatch
                .request
                .dependency_cache_refs
                .iter()
                .map(|dependency| dependency.dependency_request_id.as_str())
                .collect::<Vec<_>>(),
            dependencies.iter().map(String::as_str).collect::<Vec<_>>()
        );

        let mut scenario = super::super::qa::scenario(&dispatch.request);
        // Both nested web apps use an explicit Save button; the shared simple
        // fixture scenario's Enter action is for the single-target forms.
        if target_root == "apps/web" || target_root == "services/api" {
            scenario.steps[2].driver_action = crate::runtime_qa::QaDriverActionV1::BrowserClick {
                locator: crate::runtime_qa::QaBrowserLocatorV1::Role {
                    role: "button".into(),
                    name: "Save".into(),
                },
            };
        }
        let browser_image = super::super::candidate_browser_image(&lock);
        let service_image = lock.select(profile, "amd64").unwrap();
        super::super::qa::run(
            &docker,
            &lock,
            &browser_image,
            &service_image,
            &dispatch.request,
            &scenario,
            &dispatch.startup_recipe,
            &dispatch.artifact_upload,
            &dispatch.task_clone,
            fixture.root.path(),
            "/",
            super::super::qa::ExpectedBrowserOutcome::Pass,
        );

        for (receipt, operation) in [
            (build, RuntimeOperation::Build),
            (test, RuntimeOperation::Test),
            (start, RuntimeOperation::Start),
        ] {
            assert_eq!(receipt.operation, operation);
            assert_eq!(receipt.state, RuntimeReceiptState::Passed);
            assert_eq!(receipt.task_id, fixture.workspace.task_id);
            assert_eq!(receipt.target_id, target.target_id);
            assert_eq!(receipt.source_sha, fixture.workspace.source_sha);
            let operation_name = match operation {
                RuntimeOperation::Build => "build",
                RuntimeOperation::Test => "test",
                RuntimeOperation::Start => "start",
            };
            assert_eq!(
                receipt.recipe_id.as_deref(),
                Some(format!("polyglot-{stack}-{operation_name}").as_str())
            );
            assert_eq!(receipt.prepared_dependency_ids, dependencies);
            observed.push(receipt);
        }
    }

    assert_eq!(observed.len(), 6);
    assert!(
        observed
            .iter()
            .all(|receipt| receipt.source_sha == fixture.workspace.source_sha)
    );
}

fn session(fixture: &Fixture, lock: &ImageLock, profile: RuntimeProfile) -> ExecutionSession {
    let resolver = crate::execution::docker::backend::OwnedTaskWorkspaceResolver::new(
        &fixture.projects_root,
        fixture.workspace.clone(),
        &fixture.task_clone,
    )
    .unwrap();
    let backend = crate::execution::docker::backend::DockerExecutionBackend::connect(
        super::super::rootless_action_endpoint(),
        Arc::new(resolver),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    )
    .unwrap();
    let image = lock.select(profile, "amd64").unwrap();
    ExecutionSession::new(
        Arc::new(backend),
        fixture.workspace.clone(),
        ExecutionTarget::local(),
        ExecutionModeV1::Verification,
        image.digest.clone(),
        ResourceBudgetV1::default(),
    )
    .unwrap()
}
