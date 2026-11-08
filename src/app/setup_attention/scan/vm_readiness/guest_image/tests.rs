use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::{EvidenceCode, EvidenceState, probe_bundle_at};
use crate::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageCacheV1, GuestImageCatalogGenerationFloorV1,
    GuestImageEntryV1, GuestImageManifestTrustV1, GuestImageManifestV1, GuestImageStatusV1,
};

#[test]
fn checked_image_domain_requires_linux_amd64_host() {
    assert!(super::ready_for_host(
        crate::app::setup_attention::catalog::HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        EvidenceState::Ready,
    ));
    assert!(!super::ready_for_host(
        crate::app::setup_attention::catalog::HostKind::MacOs,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        EvidenceState::Ready,
    ));
    assert!(!super::ready_for_host(
        crate::app::setup_attention::catalog::HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Arm64,
        EvidenceState::Ready,
    ));
}

#[test]
fn bundled_catalog_stays_unverified_without_local_floor_and_cache() {
    let lock: serde_json::Value = serde_json::from_str(super::IMAGE_LOCK).unwrap();
    assert_eq!(lock["status"], "promoted");
    assert_eq!(lock["catalog_generation"], 1);
    assert!(
        lock["images"]
            .as_array()
            .is_some_and(|images| !images.is_empty())
    );

    let probe = super::probe_bundle_at(super::IMAGE_LOCK.as_bytes(), super::SIGNED_CATALOG, None);
    assert_eq!(probe.state, EvidenceState::Unknown);
    assert_eq!(
        probe.code,
        EvidenceCode::ComputerGuestImageCatalogUnverified
    );
}

#[test]
fn malformed_or_promoted_lock_never_proves_image_integrity() {
    for bytes in [
        b"{bad json}".as_slice(),
        br#"{"schema_version":1,"status":"promoted","manifest_sha256":"sha256:abc","catalog_generation":1,"images":["sha256:abc"]}"#,
        br#"{"schema_version":2,"status":"not_promoted","manifest_sha256":null,"catalog_generation":null,"images":[]}"#,
    ] {
        let probe = probe_bundle_at(bytes, b"{}", None);
        assert_eq!(probe.state, EvidenceState::Unknown);
        assert_eq!(probe.code, EvidenceCode::ComputerGuestImageCatalogUnverified);
    }
    assert_eq!(
        probe_bundle_at(&vec![b'x'; super::MAX_IMAGE_LOCK_BYTES + 1], b"{}", None).state,
        EvidenceState::Unknown
    );
}

#[test]
fn missing_catalog_floor_and_cache_cannot_become_ready() {
    let promoted = br#"{"schema_version":1,"status":"promoted","manifest_sha256":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","catalog_generation":1,"images":["sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]}"#;
    let probe = probe_bundle_at(promoted, b"{}", None);
    assert_eq!(probe.state, EvidenceState::Unknown);
    assert_eq!(
        probe.code,
        EvidenceCode::ComputerGuestImageCatalogUnverified
    );
}

#[test]
fn readiness_does_not_initialize_missing_generation_floor_or_cache() {
    let promoted = br#"{"schema_version":1,"status":"promoted","manifest_sha256":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","catalog_generation":1,"images":["sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]}"#;
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("computer-use");
    let probe = probe_bundle_at(promoted, b"{}", Some(&root));
    assert_eq!(probe.state, EvidenceState::Unknown);
    assert!(!root.exists());
}

#[test]
fn verified_catalog_lock_and_cached_image_produce_ready_evidence() {
    let artifact = b"synthetic guest artifact";
    let image_digest = format!("sha256:{:x}", Sha256::digest(artifact));
    let signer = SigningKey::from_bytes(&[91; 32]);
    let signer_id = "00000000-0000-4000-8000-000000000091";
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut manifest = GuestImageManifestV1 {
        schema_version: 1,
        catalog_version: "computer-use-images-v1-test.1".into(),
        catalog_generation: 1,
        issued_at_unix_ms: now - 1,
        expires_at_unix_ms: now + 60_000,
        signer_key_id: signer_id.into(),
        images: vec![GuestImageEntryV1 {
            architecture: ComputerGuestArchitectureV1::X86_64,
            image_digest: image_digest.clone(),
            image_size_bytes: 2 * 1024 * 1024 * 1024,
            artifact_size_bytes: artifact.len() as u64,
            artifact_name: "koolade-computer-use-guest-x86_64.qcow2".into(),
            guest_release: "debian-bookworm-test".into(),
            source_revision: "a".repeat(40),
            build_recipe_sha256: image_digest.clone(),
            sbom_sha256: image_digest.clone(),
            build_attestation_sha256: image_digest.clone(),
            companion_protocol_version: 1,
            companion_sha256: image_digest.clone(),
            status: GuestImageStatusV1::Approved,
        }],
        signature_hex: "0".repeat(128),
    };
    manifest.signature_hex = manifest
        .signing_payload_bytes()
        .map(|payload| {
            signer
                .sign(&payload)
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        })
        .unwrap();
    let catalog_bytes = serde_json::to_vec(&manifest).unwrap();
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(signer_id.into(), signer.verifying_key())],
        1,
    )
    .unwrap();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("computer-use");
    std::fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let _floor = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        root.join("catalog-floor"),
        1,
    )
    .unwrap();
    let image_cache_root = root.join("images");
    let _cache = GuestImageCacheV1::new(image_cache_root.clone()).unwrap();
    let digest_filename = image_digest.strip_prefix("sha256:").unwrap();
    std::fs::write(
        image_cache_root.join(format!("{digest_filename}.qcow2")),
        artifact,
    )
    .unwrap();
    let lock = serde_json::json!({
        "schema_version": 1,
        "status": "promoted",
        "manifest_sha256": manifest.digest().unwrap(),
        "catalog_generation": manifest.catalog_generation,
        "images": [image_digest],
    });
    let probe = super::probe_bundle_with_trust(
        &serde_json::to_vec(&lock).unwrap(),
        &catalog_bytes,
        Some(&root),
        &trust,
    );
    assert_eq!(probe.state, EvidenceState::Ready);
    assert_eq!(probe.code, EvidenceCode::ComputerGuestImageCatalogVerified);
}
