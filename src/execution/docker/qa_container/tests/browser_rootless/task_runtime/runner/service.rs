use crate::execution::docker::images::RuntimeProfile;

use super::super::setup::Fixture;

pub(crate) fn run(
    fixture: Fixture,
    build_test_profile: RuntimeProfile,
    dependency_request_id: Option<String>,
) {
    let endpoint = super::super::rootless_action_endpoint();
    let _action_socket_environment = super::super::allow_rootless_action_socket(&endpoint);
    let docker = endpoint.connect().unwrap();
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
        "task-runtime QA must run on rootless Docker"
    );

    let lock = super::super::candidate_lock();
    let service_profile = match build_test_profile {
        RuntimeProfile::Go => RuntimeProfile::Go,
        RuntimeProfile::Node => RuntimeProfile::Node,
        _ => RuntimeProfile::Python,
    };
    let service_image = lock.select(service_profile, "amd64").unwrap();
    let expected_dependencies = fixture
        .dependency_cache
        .as_ref()
        .map(|cache| vec![cache.request_id.clone()])
        .unwrap_or_default();
    assert_eq!(
        dependency_request_id.as_deref(),
        fixture
            .dependency_cache
            .as_ref()
            .map(|cache| cache.request_id.as_str())
    );

    let build_test_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let build_test_session =
        super::runtime_session(&fixture, &lock, build_test_profile, &build_test_session_id);
    if let Some(cache) = &fixture.dependency_cache {
        build_test_session
            .stage_dependency_cache_for_request(&cache.request_id, cache.snapshot.clone())
            .unwrap();
    }
    build_test_session
        .record_prepared_dependency_ids(&expected_dependencies)
        .unwrap();
    assert_eq!(
        build_test_session.prepared_dependency_ids(),
        expected_dependencies
    );

    let runtime_store = crate::project_runtime::ProjectRuntimeStore::new(&fixture.planning);
    let executor = crate::project_runtime::ProjectRuntimeExecutor::new(&runtime_store);
    let build_receipt = executor
        .execute(
            &build_test_session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Build,
        )
        .unwrap()
        .remove(0);
    let test_receipt = executor
        .execute(
            &build_test_session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Test,
        )
        .unwrap()
        .remove(0);
    assert_eq!(
        test_receipt.state,
        crate::project_runtime::RuntimeReceiptState::Passed,
        "runtime Test did not pass (exit_code={:?}, reviewed_source_match={}); diagnostic: {}",
        test_receipt.exit_code,
        test_receipt.source_sha == fixture.workspace.source_sha,
        test_failure_diagnostic(&fixture, &test_receipt),
    );
    let start_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let start_session = super::runtime_session(&fixture, &lock, service_profile, &start_session_id);
    if let Some(cache) = &fixture.dependency_cache {
        start_session
            .stage_dependency_cache_for_request(&cache.request_id, cache.snapshot.clone())
            .unwrap();
    }
    start_session
        .record_prepared_dependency_ids(&expected_dependencies)
        .unwrap();
    let start_receipt = executor
        .execute(
            &start_session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Start,
        )
        .unwrap()
        .remove(0);
    for (receipt, operation, recipe_id) in [
        (
            &build_receipt,
            crate::project_runtime::RuntimeOperation::Build,
            fixture.build_recipe_id.as_str(),
        ),
        (
            &test_receipt,
            crate::project_runtime::RuntimeOperation::Test,
            fixture.test_recipe_id.as_str(),
        ),
        (
            &start_receipt,
            crate::project_runtime::RuntimeOperation::Start,
            fixture.start_recipe_id.as_str(),
        ),
    ] {
        assert_eq!(receipt.operation, operation);
        assert_eq!(receipt.recipe_id.as_deref(), Some(recipe_id));
        assert_eq!(
            receipt.state,
            crate::project_runtime::RuntimeReceiptState::Passed
        );
        assert_eq!(receipt.task_id, fixture.workspace.task_id);
        assert_eq!(receipt.target_id, fixture.target_id);
        assert_eq!(receipt.source_sha, fixture.workspace.source_sha);
        assert_eq!(receipt.prepared_dependency_ids, expected_dependencies);
    }
    assert!(!build_receipt.artifact_hashes.is_empty());
    match build_test_profile {
        RuntimeProfile::Python => {
            assert!(build_receipt.artifact_hashes.contains_key("dist/app.py"));
            assert!(
                build_receipt
                    .artifact_hashes
                    .contains_key("dist/index.html")
            );
        }
        RuntimeProfile::Rust => assert!(
            build_receipt
                .artifact_hashes
                .contains_key("target/release/runtime-qa-rust-web")
        ),
        RuntimeProfile::Node => {
            assert!(build_receipt.artifact_hashes.contains_key("dist/server.ts"));
            assert!(
                build_receipt
                    .artifact_hashes
                    .contains_key("dist/index.html")
            );
        }
        RuntimeProfile::Go => assert!(
            build_receipt
                .artifact_hashes
                .contains_key("dist/runtime-qa-go-api")
        ),
        _ => unreachable!("runtime fixture only uses language-specific profiles"),
    }
    start_session.stop_runtime_processes().unwrap();

    let project = fixture
        .project
        .as_ref()
        .expect("Web and headless fixtures have reviewed project state");
    let task = fixture
        .lifecycle_task
        .as_ref()
        .expect("Web and headless fixtures have reviewed lifecycle tasks");
    let dispatch: crate::app::root::test_runtime_qa::RootlessTaskQaDispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_task_qa_dispatch(project, task)
            .expect("production request preparation and artifact dispatch accept exact receipts");
    assert_eq!(dispatch.request.code_head_sha, fixture.workspace.source_sha);
    assert_eq!(
        dispatch.request.target_id.as_deref(),
        Some(fixture.target_id.as_str())
    );
    assert_eq!(
        dispatch
            .request
            .dependency_cache_refs
            .iter()
            .map(|dependency| dependency.dependency_request_id.clone())
            .collect::<Vec<_>>(),
        expected_dependencies
    );
    let scenario = super::super::qa::scenario(&dispatch.request);
    let browser_image = super::super::candidate_browser_image(&lock);
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
        &fixture.readiness_path,
        if fixture.expect_browser_failure {
            super::super::qa::ExpectedBrowserOutcome::Fail
        } else {
            super::super::qa::ExpectedBrowserOutcome::Pass
        },
    );
}

fn test_failure_diagnostic(
    fixture: &Fixture,
    receipt: &crate::project_runtime::RuntimeReceiptV1,
) -> String {
    let Some(execution_id) = receipt.log_reference.as_deref() else {
        return "execution output is unavailable".into();
    };
    let Ok(cursor) = crate::persistence::execution_records::journal_cursor(
        &fixture.workspace.task_id,
        execution_id,
    ) else {
        return "execution output is unavailable".into();
    };
    let Ok(page) =
        crate::execution::contract::EventPageRequestV1::new(cursor.saturating_sub(128), 128)
    else {
        return "execution output page is invalid".into();
    };
    let Ok(events) = crate::persistence::execution_records::load_events(
        &fixture.workspace.task_id,
        execution_id,
        page,
    ) else {
        return "execution output is unavailable".into();
    };
    let diagnostics = events
        .iter()
        .filter_map(|event| match &event.event {
            crate::execution::contract::ExecutionEventKindV1::Stderr { chunk } => Some(chunk),
            _ => None,
        })
        .flat_map(|chunk| chunk.lines())
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("error:")
                || line.contains("panicked")
                || line.contains("assertion failed")
                || line.contains("failed")
        })
        .rev()
        .take(3)
        .map(crate::runtime_qa::diagnostic_redaction::sanitized_reason)
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        "no bounded Cargo error lines were recorded".into()
    } else {
        diagnostics.join(" | ")
    }
}
