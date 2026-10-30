use super::*;
#[path = "run/finalize.rs"]
mod finalize;
#[path = "run/primary.rs"]
mod primary;

/// Boundary: app-owned startup uploads a bounded Python web artifact and a
/// integrity-verified npm cache snapshot into service tmpfs, proves the service process can
/// read the cache as its locked user, provisions its internal preview relay,
/// runs the pinned browser and, in candidate CI, the headless service driver
/// through the app-owned relay; rejects requests to metadata, host-local and
/// off-origin destinations; attempts to read synthetic credentials under the
/// runner's HOME, connect to its actual Docker socket, and launch a canary from
/// writable no-exec tmpfs in the live browser container; and cleans owned
/// resources.
/// Unit tests cannot prove rootless Docker extraction, preview reachability,
/// browser/API execution, request containment, runtime access to host resources,
/// or container cleanup.
/// External dependency: rootless Docker plus a pinned browser image. Candidate
/// CI sets `KOOLADE_QA_REQUIRE_SERVICE_DRIVER=1` so this exact path also proves
/// API-only support on the freshly built image; compatibility smoke uses the
/// reviewed base image and exercises only its supported browser mode. Setup
/// cost: one candidate image build, two small synthetic caches, one isolated
/// host-HOME subdirectory, a second preview, two browser drivers, and
/// cancellation/security probes.
#[tokio::test]
#[ignore = "requires rootless Docker and a locally loaded browser QA candidate"]
async fn rootless_app_owned_browser_preview_and_driver_produce_evidence_and_clean_up() {
    let image_reference = std::env::var("KOOLADE_QA_BROWSER_IMAGE")
        .expect("candidate image reference is provided by the rootless QA workflow");
    let digest = std::env::var("KOOLADE_QA_BROWSER_DIGEST")
        .expect("candidate image digest is provided by the rootless QA workflow");
    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
    let info = docker.info().await.unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "browser application lifecycle must run on rootless Docker"
    );

    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("browser_qa")
        .and_then(|variants| {
            variants
                .iter_mut()
                .find(|variant| variant.platform == "linux/amd64")
        })
        .expect("browser QA candidate is present in the reviewed image lock");
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    let browser_image = lock.select(RuntimeProfile::BrowserQa, "amd64").unwrap();
    let service_image = lock.select(RuntimeProfile::Python, "amd64").unwrap();

    let mut request = {
        let mut request = request();
        request.bounded_limits = QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 90,
            max_evidence_bytes: 16 * 1024 * 1024,
            max_screenshots: 4,
        };
        request.driver_profile_version = browser_image.tool_versions["driver"].clone();
        request
    };
    let planning_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    request.planning_item_uid = Some(planning_uid.clone());
    let cache_root = tempfile::tempdir().unwrap();
    std::fs::write(
        cache_root.path().join("package.tgz"),
        b"approved offline browser cache fixture",
    )
    .unwrap();
    let cache = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_root.path(),
        &planning_uid,
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        &request.parent_task_uid,
        "sha256:synthetic-lockfile",
        "amd64",
        "local",
    )
    .unwrap();
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "python-preview".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["/workspace/server.py".into()],
        allowed_env_keys: vec!["HOST".into(), "PORT".into()],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 90,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![],
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::ApprovedServicePorts,
        evidence_refs: vec!["user-approval:rootless-python-preview-fixture".into()],
    };
    let recipe_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&recipe).unwrap())
    );
    request.startup_recipe_ref = recipe.recipe_id.clone();
    request.startup_recipe_sha256 = recipe_hash;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/browser/server.py");
    let artifact_root = tempfile::tempdir().unwrap();
    let host_identity_root = super::security::synthetic_host_identity_root();
    std::fs::copy(fixture, artifact_root.path().join("server.py")).unwrap();
    let artifact_hash = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(artifact_root.path().join("server.py")).unwrap())
    );
    request.build_artifact_sha256 = artifact_hash.clone();
    let (artifact_upload, _) = snapshot_selected_files(
        artifact_root.path(),
        &["server.py".into()],
        &BTreeMap::from([("server.py".into(), artifact_hash)]),
        1024 * 1024,
    )
    .unwrap();

    let mut scenario = scenario::browser_scenario(&request);
    // This test double reviews functional traces and metadata, not pixels.
    // Pixel expectations are covered by the fail-closed visual-review test.
    scenario.visual_expectations.clear();
    request.validate().unwrap();
    scenario.validate(&request).unwrap();
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let mut resources = resources();
    resources.process_count = 128;
    let preview_spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &browser_image,
        service_image: &service_image,
        recipe: &recipe,
        artifact_upload: &artifact_upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    let preview = super::super::service::start_web_preview_with_lock(
        &docker,
        preview_spec,
        &mut cancel,
        &lock,
    )
    .await
    .unwrap();
    validate_preview_relay_network(
        &docker,
        &request,
        &preview.service_lease,
        &preview.relay_binding,
    )
    .await
    .unwrap();

    let primary_result = primary::run(primary::PrimaryDriverContext {
        docker: &docker,
        request: &request,
        scenario: &scenario,
        browser_image: &browser_image,
        lock: &lock,
        artifact_root: artifact_root.path(),
        host_identity_root: host_identity_root.path(),
        artifact_upload: &artifact_upload,
        preview: &preview,
        resources: &resources,
        cancel: &mut cancel,
    })
    .await;
    let driver_result = match primary_result {
        Ok(primary_result) => {
            if let Some(driver_id) = primary_result.driver_container_id {
                cleanup_driver_container(&docker, &driver_id).await.unwrap();
            }
            primary_result.output
        }
        Err(error) => Err(error),
    };
    let headless_service_result = if service_driver::required_for_candidate() {
        Some(
            service_driver::run(
                &docker,
                &request,
                &scenario,
                &browser_image,
                &lock,
                artifact_root.path(),
                &artifact_upload,
                &preview,
                &resources,
                &mut cancel,
            )
            .await,
        )
    } else {
        None
    };
    let origin_escape_result = redirect::assert_origin_escape_cases(
        &docker,
        &request,
        &browser_image,
        &lock,
        &artifact_root,
        &artifact_upload,
        &preview,
        &resources,
        &mut cancel,
    )
    .await;
    let seeded_defect_result = defects::assert_seeded_wrong_destination_is_reported(
        &docker,
        &request,
        &browser_image,
        &lock,
        &artifact_root,
        &artifact_upload,
        &preview,
        &resources,
        &mut cancel,
    )
    .await;
    let cancellation_result = cancellation::assert_cancelled_browser_navigation_is_cleaned(
        &docker,
        &request,
        &browser_image,
        &lock,
        &artifact_root,
        &artifact_upload,
        &preview,
        &resources,
    )
    .await;
    let concurrent_runs_result =
        concurrency::assert_parallel_browser_runs_are_isolated(concurrency::ParallelRunContext {
            docker: &docker,
            request: &request,
            scenario: &scenario,
            browser_image: &browser_image,
            service_image: &service_image,
            lock: &lock,
            artifact_root: &artifact_root,
            artifact_upload: &artifact_upload,
            recipe: &recipe,
            preview: &preview,
            resources: &resources,
        })
        .await;
    finalize::finish(finalize::FinalizationContext {
        docker: &docker,
        request: &request,
        scenario,
        preview: &preview,
        driver_result,
        origin_escape_result,
        seeded_defect_result,
        cancellation_result,
        concurrent_runs_result,
        headless_service_result,
        evidence_root: artifact_root.path(),
    })
    .await;
}
