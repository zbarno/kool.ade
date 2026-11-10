use super::common::*;

#[test]
fn signed_catalog_scopes_apps_to_image_and_detects_package_tampering() {
    let catalog = signed_catalog();
    assert_eq!(
        catalog.app("text-editor").unwrap().package.package_sha256,
        APP_PACKAGE
    );
    assert!(catalog.app("/usr/bin/sh").is_none());
    let mut tampered = catalog.catalog().clone();
    tampered.apps[0].package.package_sha256 =
        "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into();
    let signer = SigningKey::from_bytes(&[7; 32]);
    let trust = GuestApplicationCatalogTrustV1::from_application_pinned_keys([(
        "00000000-0000-4000-8000-000000000001".into(),
        signer.verifying_key(),
    )])
    .unwrap();
    assert!(VerifiedGuestApplicationCatalogV1::verify(tampered, &trust).is_err());
}

#[test]
fn production_catalog_trust_uses_an_application_pinned_key() {
    assert!(GuestApplicationCatalogTrustV1::application_pinned().is_ok());
}
