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

#[test]
fn web_preview_uses_the_locked_recipe_runtime_image_instead_of_a_node_only_contract() {
    let (request, recipe, image, upload, resources) = fixture();
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    assert_eq!(body.image.as_deref(), Some(image.reference.as_str()));
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
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    assert!(validate_spec(&spec).is_err());
}
