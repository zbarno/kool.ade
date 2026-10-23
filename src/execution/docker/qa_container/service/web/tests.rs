use super::*;
use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::{images::SelectedImage, qa_container::QaContainerUpload},
    },
    project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource},
    runtime_qa::{QaBoundedLimitsV1, QaPlatform, QaRunRequestV1},
};
use sha2::Digest;

fn fixture() -> (
    QaRunRequestV1,
    ExecutionRecipeV1,
    SelectedImage,
    QaContainerUpload,
    ResourceBudgetV1,
) {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "python-preview".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python3".into(),
        argv: vec!["/workspace/server.py".into()],
        allowed_env_keys: vec!["HOST".into(), "PORT".into()],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![],
        cache_scope: None,
        prerequisite_tool_ids: vec!["python3".into()],
        network_requirement: NetworkRequirement::ApprovedServicePorts,
        evidence_refs: vec!["user-approval:fixture-python-web-server".into()],
    };
    let recipe_bytes = serde_json::to_vec(&recipe).unwrap();
    let recipe_sha = format!("{:x}", sha2::Sha256::digest(recipe_bytes));
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
        planning_item_uid: None,
        feature_uid: uid(),
        code_head_sha: "a".repeat(40),
        build_artifact_sha256: "b".repeat(64),
        complete_diff_hash: "c".repeat(64),
        review_subject_sha256: "d".repeat(64),
        approved_spec_hash: "e".repeat(64),
        requirement_ids: vec!["REQ-WEB".into()],
        acceptance_criteria: vec!["Python task preview runs in isolation.".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: recipe.recipe_id.clone(),
        startup_recipe_sha256: recipe_sha,
        execution_session_id: uid(),
        driver_profile_version: "browser-profile-v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 4,
            max_total_actions: 4,
            timeout_seconds: 30,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 2,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    };
    let image = SelectedImage {
        profile: crate::execution::docker::images::RuntimeProfile::BrowserQa,
        platform: "linux/amd64".into(),
        reference: format!(
            "ghcr.io/zbarno/koolade-runtime-qa-browser@sha256:{}",
            "a".repeat(64)
        ),
        digest: format!("sha256:{}", "a".repeat(64)),
        runtime_user: "10001:10001".into(),
        tool_versions: std::collections::BTreeMap::from([
            ("python3".into(), "3.12.3".into()),
            ("node".into(), "24.21.0".into()),
            ("npm".into(), "11.19.0".into()),
            ("playwright".into(), "1.57.0".into()),
            ("chromium".into(), "143.0".into()),
            ("driver".into(), "browser-qa-v1".into()),
            ("base".into(), "fixture".into()),
        ]),
    };
    let upload = QaContainerUpload {
        target: "/workspace".into(),
        archive: vec![1],
    };
    let resources = ResourceBudgetV1 {
        cpu_millis: 1000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 32,
        temporary_disk_bytes: 16 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    (request, recipe, image, upload, resources)
}

fn npm_cache(request: &QaRunRequestV1) -> crate::harness::resource_bridge::CacheSnapshot {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.tgz"), b"synthetic cached package").unwrap();
    crate::harness::resource_bridge::CacheSnapshot::capture(
        root.path(),
        request.planning_item_uid.as_deref().unwrap(),
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        "88888888-8888-4888-8888-888888888888",
        "sha256:synthetic-lockfile",
        "amd64",
        "local",
    )
    .unwrap()
}

#[test]
fn web_preview_uses_the_locked_recipe_runtime_image_instead_of_a_node_only_contract() {
    let (request, recipe, image, upload, resources) = fixture();
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: &[],
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    assert_eq!(body.image.as_deref(), Some(image.reference.as_str()));
    assert!(
        body.host_config.as_ref().unwrap().tmpfs.as_ref().unwrap()["/workspace"]
            .contains("uid=10001,gid=10001")
    );
    assert_eq!(
        body.entrypoint.as_deref(),
        Some(["python3".to_owned()].as_slice())
    );
    assert_eq!(
        body.cmd.as_deref(),
        Some(["/workspace/server.py".to_owned()].as_slice())
    );
    let environment = body.env.unwrap();
    assert!(environment.contains(&"PORT=4173".into()));
    assert!(environment.contains(&"HOST=0.0.0.0".into()));
    assert!(
        !environment
            .iter()
            .any(|value| value.contains("TOKEN") || value.contains("SECRET"))
    );
}

#[test]
fn preview_relay_health_checks_the_exact_task_service_path() {
    let (request, recipe, image, upload, resources) = fixture();
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: &[],
        preview_port: 4173,
        health_path: "/healthz?probe=ready",
        resources: &resources,
    };
    let body = relay_body(&spec, "task-network");
    let command = body.healthcheck.unwrap().test.unwrap();
    assert!(command[3].contains("status=>status<500"));
    assert!(command[3].contains("headers:{host:'qa-preview:8080'}"));
    assert!(command[3].contains("path:\"/healthz?probe=ready\""));
    assert_eq!(
        command.len(),
        4,
        "healthcheck target and path must be embedded in the fixed command"
    );
}

#[test]
fn web_preview_rejects_a_recipe_tool_absent_from_its_pinned_runtime_profile() {
    let (mut request, mut recipe, image, upload, resources) = fixture();
    recipe.executable_id = "go".into();
    recipe.prerequisite_tool_ids = vec!["go".into()];
    request.startup_recipe_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&recipe).unwrap())
    );
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: &[],
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    assert!(validate_spec(&spec).is_err());
}

#[test]
fn web_preview_transfers_only_request_bound_caches_into_offline_service_paths() {
    let (mut request, recipe, image, upload, resources) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache(&request);
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };

    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    let environment = body.env.unwrap();
    assert!(environment.contains(&"NPM_CONFIG_CACHE=/home/executor/.npm".into()));
    assert!(environment.contains(&"NPM_CONFIG_OFFLINE=true".into()));
    let tmpfs = body.host_config.unwrap().tmpfs.unwrap();
    assert!(tmpfs.contains_key("/home/executor/.npm"));
    let allocated_bytes = tmpfs
        .values()
        .map(|mount| {
            mount
                .split(',')
                .find_map(|option| option.strip_prefix("size="))
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
        .sum::<u64>();
    assert!(allocated_bytes <= resources.temporary_disk_bytes);
}

#[test]
fn web_preview_rejects_cache_archives_not_bound_to_its_immutable_request() {
    let (mut request, recipe, image, upload, resources) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache(&request);
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: cache.cache_ref.clone(),
    }];
    request.dependency_cache_refs[0].cache_ref.target_arch = "arm64".into();
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };

    assert!(validate_spec(&spec).is_err());
}

#[test]
fn dependency_cache_upload_tar_is_readable_by_the_qa_service_user() {
    let (mut request, _, image, _, _) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache(&request);
    let archive = dependency_cache::archive_for_runtime_user(&cache, &image.runtime_user).unwrap();
    let mut tar = tar::Archive::new(archive.as_slice());
    let mut entries = tar.entries().unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.header().uid().unwrap(), 10001);
    assert_eq!(entry.header().gid().unwrap(), 10001);
    assert_eq!(entry.header().mode().unwrap() & 0o777, 0o600);
    assert_eq!(entry.size(), b"synthetic cached package".len() as u64);
}
