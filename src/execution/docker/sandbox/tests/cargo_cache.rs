use super::*;

#[test]
fn rust_container_uses_the_staged_cargo_cache() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::Rust, "amd64")
        .unwrap();
    let mut request = request(ExecutionModeV1::Implementation);
    request.image_digest = image.digest.clone();
    let body = create_body(&request, &workspace, &identity(), &image).unwrap();

    assert!(
        body.env
            .unwrap()
            .iter()
            .any(|value| value == "CARGO_HOME=/home/executor/.cargo")
    );
}
