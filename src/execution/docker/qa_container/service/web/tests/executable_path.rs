use super::*;
use sha2::Digest;

#[test]
fn web_preview_launches_a_receipt_bound_target_relative_binary_without_an_image_tool_id() {
    let (mut request, mut recipe, image, upload, resources) = fixture();
    recipe.executable_id = "dist/server".into();
    recipe.argv.clear();
    recipe.prerequisite_tool_ids.clear();
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
        health_path: "/health",
        resources: &resources,
    };

    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    assert_eq!(
        body.entrypoint.as_deref(),
        Some(["dist/server".to_owned()].as_slice())
    );
    assert!(body.cmd.as_deref().unwrap().is_empty());
    assert_eq!(body.working_dir.as_deref(), Some("/workspace"));
}
