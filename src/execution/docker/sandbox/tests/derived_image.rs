use super::*;

#[test]
fn only_prepared_registered_derived_images_bypass_the_bundled_lock() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let mut derived = selected_image();
    derived.digest = format!("sha256:{}", "d".repeat(64));
    derived.reference = derived.digest.clone();
    let mut request = request(ExecutionModeV1::Implementation);
    request.image_digest = derived.digest.clone();
    let approval = super::super::super::backend::PreparedDerivedImage::test_approval(
        derived.clone(),
        selected_image().digest,
    );

    assert!(create_body(&request, &workspace, &identity(), &derived).is_err());
    assert!(
        create_body_with_derived_approval(
            &request,
            &workspace,
            &identity(),
            &derived,
            Some(&approval),
        )
        .is_ok()
    );
    let mut tampered = derived.clone();
    tampered.reference = "unregistered:latest".into();
    assert!(
        create_body_with_derived_approval(
            &request,
            &workspace,
            &identity(),
            &tampered,
            Some(&approval),
        )
        .is_err()
    );
    request.image_digest = selected_image().digest;
    assert!(
        create_body_with_derived_approval(
            &request,
            &workspace,
            &identity(),
            &derived,
            Some(&approval),
        )
        .is_err()
    );
}
