use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::super::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, verify_guest_image_catalog,
};
use crate::computer_use::guest_image::ImageCacheErrorV1;
use crate::computer_use::guest_image::{
    ComputerImageDownloadApprovalV1, GuestImageCacheV1, GuestImageCatalogGenerationFloorV1,
};

#[test]
fn catalog_approved_image_can_be_downloaded_before_a_session_exists() {
    let artifact = b"approved image bytes";
    let image_digest = format!("sha256:{:x}", Sha256::digest(artifact));
    let signer = SigningKey::from_bytes(&[73; 32]);
    let signer_id = "00000000-0000-4000-8000-000000000073";
    let mut manifest = GuestImageManifestV1 {
        schema_version: 1,
        catalog_version: "computer-use-images-v1-2026.11.06".into(),
        catalog_generation: 7,
        issued_at_unix_ms: 1_000_000,
        expires_at_unix_ms: 2_000_000,
        signer_key_id: signer_id.into(),
        images: vec![GuestImageEntryV1 {
            architecture: ComputerGuestArchitectureV1::X86_64,
            image_digest: image_digest.clone(),
            image_size_bytes: 2 * 1024 * 1024 * 1024,
            artifact_size_bytes: artifact.len() as u64,
            artifact_name: "koolade-computer-use-guest-x86_64.qcow2".into(),
            guest_release: "debian-bookworm-test".into(),
            source_revision: "b".repeat(40),
            build_recipe_sha256: image_digest.clone(),
            sbom_sha256: image_digest.clone(),
            build_attestation_sha256: image_digest.clone(),
            companion_protocol_version: 1,
            companion_sha256: image_digest,
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
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(signer_id.into(), signer.verifying_key())],
        1,
    )
    .unwrap();
    let catalog =
        verify_guest_image_catalog(&serde_json::to_vec(&manifest).unwrap(), &trust, 1_050_000)
            .unwrap();
    let verified_image = catalog
        .approved_image_for_download(ComputerGuestArchitectureV1::X86_64)
        .unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(cache_root.path().to_path_buf()).unwrap();
    let floor = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        cache_root.path().join("catalog-floor"),
        1,
    )
    .unwrap();
    let approval = ExactImageApproval {
        manifest_digest: catalog.digest().to_owned(),
        image_digest: verified_image.entry().image_digest.clone(),
    };
    let mut denied_reader = std::io::Cursor::new(artifact);
    let denied = ExactImageApproval {
        manifest_digest: approval.manifest_digest.clone(),
        image_digest: "sha256:".to_owned() + &"0".repeat(64),
    };
    assert_eq!(
        cache.install_approved_catalog_reader(
            &verified_image,
            &floor,
            1_050_000,
            &denied,
            &mut denied_reader,
        ),
        Err(ImageCacheErrorV1::ApprovalDenied)
    );
    assert_eq!(denied_reader.position(), 0);

    let mut expired_reader = std::io::Cursor::new(artifact);
    assert_eq!(
        cache.install_approved_catalog_reader(
            &verified_image,
            &floor,
            2_000_000,
            &approval,
            &mut expired_reader,
        ),
        Err(ImageCacheErrorV1::CatalogExpired)
    );
    assert_eq!(expired_reader.position(), 0);

    let cached = cache
        .install_approved_catalog_reader(
            &verified_image,
            &floor,
            1_050_000,
            &approval,
            &mut std::io::Cursor::new(artifact),
        )
        .unwrap();
    assert_eq!(cached.manifest_digest(), catalog.digest());
    assert_eq!(cached.manifest_signer_key_id(), signer_id);
    assert_eq!(std::fs::read(cached.path()).unwrap(), artifact);
    floor.accept_generation(8).unwrap();
    let mut replayed = std::io::Cursor::new(artifact);
    assert_eq!(
        cache.install_approved_catalog_reader(
            &verified_image,
            &floor,
            1_050_000,
            &approval,
            &mut replayed,
        ),
        Err(ImageCacheErrorV1::CatalogRollback)
    );
    assert_eq!(replayed.position(), 0);
}

struct ExactImageApproval {
    manifest_digest: String,
    image_digest: String,
}

impl ComputerImageDownloadApprovalV1 for ExactImageApproval {
    fn authorize_download(
        &self,
        manifest_digest: &str,
        image_digest: &str,
    ) -> Result<(), ImageCacheErrorV1> {
        if manifest_digest == self.manifest_digest && image_digest == self.image_digest {
            Ok(())
        } else {
            Err(ImageCacheErrorV1::ApprovalDenied)
        }
    }
}
