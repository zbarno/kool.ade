use super::*;

fn snapshot() -> crate::harness::resource_bridge::CacheSnapshot {
    let root = tempfile::tempdir().expect("cache root");
    std::fs::write(root.path().join("package-lock.json"), b"synthetic cache").expect("cache entry");
    crate::harness::resource_bridge::CacheSnapshot::capture(
        root.path(),
        "11111111-1111-4111-8111-111111111111",
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        "22222222-2222-4222-8222-222222222222",
        "sha256:synthetic-integrity",
        "amd64",
        "33333333-3333-4333-8333-333333333333",
    )
    .expect("verified cache snapshot")
}

#[test]
fn retry_snapshot_requires_the_authorized_worker_and_architecture() {
    let snapshot = snapshot();
    let original_worker = "33333333-3333-4333-8333-333333333333";
    assert!(validate_retry_snapshot_scope(original_worker, Some("amd64"), &snapshot).is_ok());
    assert!(
        validate_retry_snapshot_scope(
            "44444444-4444-4444-8444-444444444444",
            Some("amd64"),
            &snapshot
        )
        .is_err()
    );
    assert!(validate_retry_snapshot_scope(original_worker, Some("arm64"), &snapshot).is_err());
    assert!(validate_retry_snapshot_scope(original_worker, None, &snapshot).is_err());
}

#[test]
fn dependency_request_ids_require_the_canonical_prefix_and_uuid() {
    use crate::execution::contract::validate_dependency_request_id as validate;

    assert!(validate("dependency-00000000-0000-4000-8000-000000000001").is_ok());
    assert!(validate("00000000-0000-4000-8000-000000000001").is_ok());
    assert!(validate("dependency-00000000-0000-4000-8000-00000000000A").is_err());
    assert!(validate("dependency-not-a-uuid").is_err());
}
