use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::super::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, verify_guest_image_catalog,
    verify_guest_image_manifest,
};
use crate::computer_use::guest_image::{GuestImageCacheV1, GuestImageCatalogGenerationFloorV1};
use crate::computer_use::{ComputerSessionV1, guest_image::ImageCacheErrorV1};

#[test]
fn revoked_and_expired_catalog_entries_fail_closed() {
    let artifact = b"fake qcow2";
    let digest = format!("sha256:{:x}", Sha256::digest(artifact));
    let signer = SigningKey::from_bytes(&[71; 32]);
    let signer_id = "00000000-0000-4000-8000-000000000071";
    let controller = SigningKey::from_bytes(&[42; 32]);
    let controller_id = "00000000-0000-4000-8000-000000000011";
    let mut catalog = GuestImageManifestV1 {
        schema_version: 1,
        catalog_version: "computer-use-images-v1-2026.11.06".into(),
        catalog_generation: 3,
        issued_at_unix_ms: 1_000_000,
        expires_at_unix_ms: 2_000_000,
        signer_key_id: signer_id.into(),
        images: vec![GuestImageEntryV1 {
            architecture: ComputerGuestArchitectureV1::X86_64,
            image_digest: digest.clone(),
            image_size_bytes: 2 * 1024 * 1024 * 1024,
            artifact_size_bytes: artifact.len() as u64,
            artifact_name: "koolade-computer-use-guest-x86_64.qcow2".into(),
            guest_release: "debian-bookworm-test".into(),
            source_revision: "a".repeat(40),
            build_recipe_sha256: digest.clone(),
            sbom_sha256: digest.clone(),
            build_attestation_sha256: digest.clone(),
            companion_protocol_version: 1,
            companion_sha256: digest,
            status: GuestImageStatusV1::Revoked,
        }],
        signature_hex: "0".repeat(128),
    };
    catalog.signature_hex = catalog
        .signing_payload_bytes()
        .map(|bytes| {
            signer
                .sign(&bytes)
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        })
        .unwrap();

    let mut session: ComputerSessionV1 = serde_json::from_str(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../../tests/fixtures/computer_use/contracts-v1.json"
        ))
        .unwrap()["session"]
            .to_string(),
    )
    .unwrap();
    session.image_digest = catalog.images[0].image_digest.clone();
    session.image_manifest_digest = catalog.digest().unwrap();
    session.image_signer_key_id = signer_id.into();
    session.attest(controller_id.into(), &controller).unwrap();
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(signer_id.into(), signer.verifying_key())],
        1,
    )
    .unwrap();
    let bytes = serde_json::to_vec(&catalog).unwrap();
    let verified_catalog = verify_guest_image_catalog(&bytes, &trust, 1_050_000).unwrap();
    assert_eq!(verified_catalog.catalog_generation(), 3);
    assert_eq!(verified_catalog.approved_images().count(), 0);
    assert!(
        verified_catalog
            .approved_image_for_download(ComputerGuestArchitectureV1::X86_64)
            .is_none()
    );
    let cache_root = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(cache_root.path().to_path_buf()).unwrap();
    let floor = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        cache_root.path().join("catalog-floor"),
        1,
    )
    .unwrap();
    assert_eq!(
        cache.verify_cached_catalog_image_at(
            &bytes,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            1_050_000,
        ),
        Err(ImageCacheErrorV1::ImageNotCatalogued)
    );
    catalog.images[0].status = GuestImageStatusV1::Approved;
    catalog.signature_hex = catalog
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
    let approved_bytes = serde_json::to_vec(&catalog).unwrap();
    let image_digest = catalog.images[0]
        .image_digest
        .strip_prefix("sha256:")
        .unwrap();
    std::fs::write(
        cache_root.path().join(format!("{image_digest}.qcow2")),
        artifact,
    )
    .unwrap();
    let cached = cache
        .verify_cached_catalog_image_at(
            &approved_bytes,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            1_050_000,
        )
        .unwrap();
    assert_eq!(cached.entry().status, GuestImageStatusV1::Approved);
    let rotated_key = SigningKey::from_bytes(&[72; 32]);
    let rotated_trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(signer_id.into(), rotated_key.verifying_key())],
        1,
    )
    .unwrap();
    assert_eq!(
        cache.verify_cached_catalog_image_at(
            &approved_bytes,
            &rotated_trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            1_050_000,
        ),
        Err(ImageCacheErrorV1::UntrustedManifestSigner)
    );
    floor.accept_generation(4).unwrap();
    assert_eq!(
        cache.verify_cached_catalog_image_at(
            &approved_bytes,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            1_050_000,
        ),
        Err(ImageCacheErrorV1::CatalogRollback)
    );
    assert_eq!(
        cache.verify_cached_catalog_image_at(
            &approved_bytes,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            2_000_000,
        ),
        Err(ImageCacheErrorV1::CatalogExpired)
    );
    assert_eq!(
        verify_guest_image_manifest(
            &bytes,
            &trust,
            &session,
            1_050_000,
            ComputerGuestArchitectureV1::X86_64,
            controller_id,
            &controller.verifying_key(),
        )
        .unwrap_err(),
        ImageCacheErrorV1::ImageRevoked
    );
    assert_eq!(
        verify_guest_image_manifest(
            &bytes,
            &trust,
            &session,
            2_000_000,
            ComputerGuestArchitectureV1::X86_64,
            controller_id,
            &controller.verifying_key(),
        )
        .unwrap_err(),
        ImageCacheErrorV1::CatalogExpired
    );
    assert_eq!(
        verify_guest_image_manifest(
            &bytes,
            &trust,
            &session,
            2_000_001,
            ComputerGuestArchitectureV1::X86_64,
            controller_id,
            &controller.verifying_key(),
        )
        .unwrap_err(),
        ImageCacheErrorV1::CatalogExpired
    );
}
