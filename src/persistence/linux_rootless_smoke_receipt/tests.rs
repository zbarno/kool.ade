use super::*;
use crate::execution::docker::images::RuntimeProfile;

fn image() -> crate::execution::docker::images::SelectedImage {
    crate::execution::docker::images::SelectedImage {
        profile: RuntimeProfile::General,
        platform: "linux/amd64".into(),
        reference: "ghcr.io/example/runtime:locked".into(),
        digest: format!("sha256:{}", "a".repeat(64)),
        runtime_user: "10001:10001".into(),
        tool_versions: Default::default(),
    }
}

fn receipt() -> RootlessSmokeReceiptV1 {
    let image = image();
    RootlessSmokeReceiptV1 {
        schema_version: SCHEMA_VERSION,
        user_id: current_user_id(),
        endpoint_fingerprint: "b".repeat(64),
        daemon_fingerprint: "c".repeat(64),
        daemon_version: "29.0.0".into(),
        image_reference: image.reference,
        image_digest: image.digest,
        image_platform: image.platform,
        completed_at_unix_ms: 10,
    }
}

#[test]
fn receipt_requires_the_same_socket_daemon_and_locked_image() {
    let receipt = receipt();
    let image = image();
    assert!(receipt.matches(&"b".repeat(64), &"c".repeat(64), "29.0.0", &image));
    assert!(!receipt.matches(&"d".repeat(64), &"c".repeat(64), "29.0.0", &image));
    assert!(!receipt.matches(&"b".repeat(64), &"d".repeat(64), "29.0.0", &image));
    assert!(!receipt.matches(&"b".repeat(64), &"c".repeat(64), "29.1.0", &image));

    let mut other_image = image.clone();
    other_image.digest = format!("sha256:{}", "e".repeat(64));
    assert!(!receipt.matches(&"b".repeat(64), &"c".repeat(64), "29.0.0", &other_image));
}

#[test]
fn receipt_rejects_malformed_hashes_images_and_foreign_users() {
    let mut invalid = receipt();
    invalid.endpoint_fingerprint = "not-a-fingerprint".into();
    assert!(invalid.validate().is_err());

    let mut invalid = receipt();
    invalid.image_platform = "linux/386".into();
    assert!(invalid.validate().is_err());

    let mut invalid = receipt();
    invalid.user_id = current_user_id().saturating_add(1);
    assert!(!invalid.matches(&"b".repeat(64), &"c".repeat(64), "29.0.0", &image()));
}
