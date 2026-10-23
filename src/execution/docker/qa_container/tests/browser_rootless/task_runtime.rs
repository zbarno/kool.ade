use super::*;
use std::sync::Arc;

#[path = "task_runtime/qa.rs"]
mod qa;
#[path = "task_runtime/setup.rs"]
mod setup;

/// Boundary: a user-approved Python task target builds and tests inside the
/// app-owned rootless ExecutionSession, records source-bound Build/Test/Start
/// receipts, and supplies only receipt-verified build files to the isolated
/// browser preview. The browser then exercises the exact approved startup
/// recipe and persists screenshot-backed evidence against that task revision.
/// Unit tests cannot prove the workspace stage-out, task container, preview
/// relay, browser interaction, and owned-resource cleanup work together.
/// External dependencies: rootless Docker and the pinned Python and browser QA
/// images. Setup cost: three short runtime recipes, one build snapshot, and one
/// bounded browser scenario; CI downloads only those two locked images.
#[test]
#[ignore = "requires rootless Docker and the loaded Python/browser QA images"]
fn rootless_approved_task_build_test_start_receipts_feed_browser_qa_artifact() {
    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
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
    let fixture = setup::Fixture::new();
    let lock = candidate_lock();
    let service_image = lock.select(RuntimeProfile::Python, "amd64").unwrap();
    let mut request = request();
    request.parent_task_uid = fixture.workspace.task_id.clone();
    request.code_head_sha = fixture.workspace.source_sha.clone();
    request.complete_diff_hash = fixture.complete_diff_hash.clone();
    request.requirement_ids = vec!["REQ-TASK-WEB-1".into()];
    request.acceptance_criteria = vec!["Submitting a non-empty message displays Saved.".into()];
    request.approved_spec_hash = qa::approved_spec_hash();
    request.review_subject_sha256 = qa::review_subject_hash(
        &request.code_head_sha,
        &request.complete_diff_hash,
        &request.approved_spec_hash,
    );
    request.execution_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    request.test_data_fixture_ref = Some("fixture:python-task-web-v1".into());

    let resolver = Arc::new(
        crate::execution::docker::backend::OwnedTaskWorkspaceResolver::new(
            &fixture.projects_root,
            fixture.workspace.clone(),
            &fixture.task_clone,
        )
        .unwrap(),
    );
    let endpoint = crate::execution::docker::engine::DockerEndpoint::from_process_environment(
        crate::execution::docker::engine::HostPlatform::Linux,
    )
    .unwrap();
    let backend = crate::execution::docker::backend::DockerExecutionBackend::connect(
        endpoint,
        resolver,
        request.execution_session_id.clone(),
    )
    .unwrap();
    let session = crate::execution::session::ExecutionSession::new(
        Arc::new(backend),
        fixture.workspace.clone(),
        crate::execution::contract::ExecutionTarget::local(),
        crate::execution::contract::ExecutionModeV1::Verification,
        service_image.digest.clone(),
        crate::execution::contract::ResourceBudgetV1::default(),
    )
    .unwrap();
    let runtime_store = crate::project_runtime::ProjectRuntimeStore::new(&fixture.planning);
    let executor = crate::project_runtime::ProjectRuntimeExecutor::new(&runtime_store);
    let build_receipt = executor
        .execute(
            &session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Build,
        )
        .unwrap()
        .remove(0);
    let test_receipt = executor
        .execute(
            &session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Test,
        )
        .unwrap()
        .remove(0);
    let start_receipt = executor
        .execute(
            &session,
            &fixture.target_id,
            crate::project_runtime::RuntimeOperation::Start,
        )
        .unwrap()
        .remove(0);
    for (receipt, operation, recipe_id) in [
        (
            &build_receipt,
            crate::project_runtime::RuntimeOperation::Build,
            "task-web-build",
        ),
        (
            &test_receipt,
            crate::project_runtime::RuntimeOperation::Test,
            "task-web-test",
        ),
        (
            &start_receipt,
            crate::project_runtime::RuntimeOperation::Start,
            "task-web-start",
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
    }
    assert_eq!(build_receipt.artifact_hashes.len(), 2);
    assert!(build_receipt.artifact_hashes.contains_key("dist/app.py"));
    assert!(
        build_receipt
            .artifact_hashes
            .contains_key("dist/index.html")
    );
    session.stop_runtime_processes().unwrap();

    let (artifact_upload, artifact_digest) = snapshot_selected_files(
        &fixture.task_clone,
        &["dist/app.py".into(), "dist/index.html".into()],
        &build_receipt.artifact_hashes,
        1024 * 1024,
    )
    .unwrap();
    request.build_artifact_sha256 = artifact_digest;
    let manifest = runtime_store.load_manifest().unwrap().unwrap();
    let start_recipe = manifest
        .user_overrides
        .iter()
        .find(|recipe| recipe.recipe_id == "task-web-start")
        .unwrap();
    request.startup_recipe_ref = start_recipe.recipe_id.clone();
    request.startup_recipe_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(start_recipe).unwrap())
    );
    request.driver_profile_version = lock
        .select(RuntimeProfile::BrowserQa, "amd64")
        .unwrap()
        .tool_versions["driver"]
        .clone();
    request.bounded_limits = QaBoundedLimitsV1 {
        max_scenarios: 1,
        max_actions_per_scenario: 8,
        max_total_actions: 8,
        timeout_seconds: 90,
        max_evidence_bytes: 16 * 1024 * 1024,
        max_screenshots: 4,
    };
    request.validate().unwrap();

    let browser_image = candidate_browser_image(&lock);
    let scenario = qa::scenario(&request);
    qa::run(
        &docker,
        &lock,
        &browser_image,
        &service_image,
        &request,
        &scenario,
        start_recipe,
        &artifact_upload,
        &fixture.task_clone,
        fixture.root.path(),
    );
}

fn candidate_lock() -> ImageLock {
    let image_reference = std::env::var("KOOLADE_QA_BROWSER_IMAGE")
        .expect("browser QA candidate reference comes from rootless CI");
    let digest = std::env::var("KOOLADE_QA_BROWSER_DIGEST")
        .expect("browser QA candidate digest comes from rootless CI");
    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("browser_qa")
        .and_then(|variants| {
            variants
                .iter_mut()
                .find(|variant| variant.platform == "linux/amd64")
        })
        .unwrap();
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    lock
}

fn candidate_browser_image(lock: &ImageLock) -> crate::execution::docker::images::SelectedImage {
    lock.select(RuntimeProfile::BrowserQa, "amd64").unwrap()
}
