//! Production boundary: signed catalog selection and approved cache installation
//! feed the host VM launcher. These unit tests verify trust and file invariants;
//! they cannot prove QEMU behavior. No network, Docker, or SSH is used; setup is low.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

#[cfg(unix)]
use super::manifest::VerifiedGuestImageV1;
use super::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, ImageCacheErrorV1, verify_guest_image_manifest,
};
#[cfg(unix)]
use super::{ComputerImageDownloadApprovalV1, GuestImageCacheV1};
use crate::computer_use::ComputerSessionV1;
#[cfg(unix)]
use std::io::Cursor;

const CATALOG_KEY_ID: &str = "00000000-0000-4000-8000-000000000021";
const CONTROLLER_KEY_ID: &str = "00000000-0000-4000-8000-000000000011";

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn entry(artifact: &[u8]) -> GuestImageEntryV1 {
    GuestImageEntryV1 {
        architecture: ComputerGuestArchitectureV1::X86_64,
        image_digest: digest(artifact),
        image_size_bytes: 2 * 1024 * 1024 * 1024,
        artifact_size_bytes: artifact.len() as u64,
        artifact_name: "koolade-computer-use-guest-x86_64.qcow2".into(),
        guest_release: "debian-bookworm-2026-11".into(),
        source_revision: "a".repeat(40),
        build_recipe_sha256: digest(b"recipe"),
        sbom_sha256: digest(b"sbom"),
        build_attestation_sha256: digest(b"attestation"),
        companion_protocol_version: 1,
        companion_sha256: digest(b"companion-contract-placeholder"),
        status: GuestImageStatusV1::Approved,
    }
}

fn signed_catalog(artifact: &[u8]) -> (GuestImageManifestV1, SigningKey) {
    let key = SigningKey::from_bytes(&[33; 32]);
    let mut manifest = GuestImageManifestV1 {
        schema_version: 1,
        catalog_version: "computer-use-images-v1-2026.11.06".into(),
        catalog_generation: 1,
        issued_at_unix_ms: 1_000_000,
        expires_at_unix_ms: 2_000_000,
        signer_key_id: CATALOG_KEY_ID.into(),
        images: vec![entry(artifact)],
        signature_hex: "0".repeat(128),
    };
    manifest.signature_hex = manifest
        .signing_payload_bytes()
        .map(|bytes| encode_hex(&key.sign(&bytes).to_bytes()))
        .unwrap();
    (manifest, key)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(unix)]
fn verified(artifact: &[u8]) -> VerifiedGuestImageV1 {
    let (manifest, catalog_key) = signed_catalog(artifact);
    let mut session: ComputerSessionV1 = serde_json::from_str(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../tests/fixtures/computer_use/contracts-v1.json"
        ))
        .unwrap()["session"]
            .to_string(),
    )
    .unwrap();
    session.image_digest = manifest.images[0].image_digest.clone();
    session.image_manifest_digest = manifest.digest().unwrap();
    session.image_signer_key_id = CATALOG_KEY_ID.into();
    let controller = SigningKey::from_bytes(&[42; 32]);
    session
        .attest(CONTROLLER_KEY_ID.into(), &controller)
        .unwrap();
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(CATALOG_KEY_ID.into(), catalog_key.verifying_key())],
        1,
    )
    .unwrap();
    verify_guest_image_manifest(
        &serde_json::to_vec(&manifest).unwrap(),
        &trust,
        &session,
        1_050_000,
        ComputerGuestArchitectureV1::X86_64,
        CONTROLLER_KEY_ID,
        &controller.verifying_key(),
    )
    .unwrap()
}

#[test]
fn catalog_signature_and_controller_session_bind_the_same_image() {
    let artifact = b"synthetic qcow2 bytes";
    let verified = verified(artifact);
    assert_eq!(verified.entry().image_digest, digest(artifact));

    let (manifest, catalog_key) = signed_catalog(artifact);
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(CATALOG_KEY_ID.into(), catalog_key.verifying_key())],
        2,
    )
    .unwrap();
    let mut session: ComputerSessionV1 = serde_json::from_str(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../tests/fixtures/computer_use/contracts-v1.json"
        ))
        .unwrap()["session"]
            .to_string(),
    )
    .unwrap();
    session.image_digest = manifest.images[0].image_digest.clone();
    session.image_manifest_digest = manifest.digest().unwrap();
    session.image_signer_key_id = CATALOG_KEY_ID.into();
    let controller = SigningKey::from_bytes(&[42; 32]);
    session
        .attest(CONTROLLER_KEY_ID.into(), &controller)
        .unwrap();
    assert_eq!(
        verify_guest_image_manifest(
            &serde_json::to_vec(&manifest).unwrap(),
            &trust,
            &session,
            1_050_000,
            ComputerGuestArchitectureV1::X86_64,
            CONTROLLER_KEY_ID,
            &controller.verifying_key(),
        )
        .unwrap_err(),
        ImageCacheErrorV1::StaleCatalog
    );

    let mut changed = manifest;
    changed.images[0].guest_release.push_str("-tampered");
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(CATALOG_KEY_ID.into(), catalog_key.verifying_key())],
        1,
    )
    .unwrap();
    assert_eq!(
        verify_guest_image_manifest(
            &serde_json::to_vec(&changed).unwrap(),
            &trust,
            &session,
            1_050_000,
            ComputerGuestArchitectureV1::X86_64,
            CONTROLLER_KEY_ID,
            &controller.verifying_key(),
        )
        .unwrap_err(),
        ImageCacheErrorV1::UntrustedManifestSigner
    );
}

#[cfg(unix)]
struct DenyDownload;

#[cfg(unix)]
impl ComputerImageDownloadApprovalV1 for DenyDownload {
    fn authorize_download(&self, _: &str, _: &str) -> Result<(), ImageCacheErrorV1> {
        Err(ImageCacheErrorV1::ApprovalDenied)
    }
}

#[test]
#[cfg(unix)]
fn cache_requires_exact_approval_and_rejects_tampered_bytes() {
    let artifact = b"synthetic qcow2 bytes";
    let verified = verified(artifact);
    let root = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(root.path().join("private-cache")).unwrap();
    let mut denied_reader = Cursor::new(artifact);
    assert_eq!(
        cache.install_approved_reader(&verified, &DenyDownload, &mut denied_reader),
        Err(ImageCacheErrorV1::ApprovalDenied)
    );
    assert_eq!(denied_reader.position(), 0, "approval happens before reads");

    let approval = AllowExact {
        manifest: verified.manifest_digest().into(),
        image: verified.entry().image_digest.clone(),
    };
    let mut changed = Cursor::new(b"modified bytes".as_slice());
    assert_eq!(
        cache.install_approved_reader(&verified, &approval, &mut changed),
        Err(ImageCacheErrorV1::DownloadLengthMismatch)
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("private-cache"))
            .unwrap()
            .count(),
        0
    );
    let mut oversized = Cursor::new(vec![0; artifact.len() + 1]);
    assert_eq!(
        cache.install_approved_reader(&verified, &approval, &mut oversized),
        Err(ImageCacheErrorV1::DownloadTooLarge)
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("private-cache"))
            .unwrap()
            .count(),
        0
    );

    let mut reader = Cursor::new(artifact);
    let cached = cache
        .install_approved_reader(&verified, &approval, &mut reader)
        .unwrap();
    assert_eq!(std::fs::read(cached.path()).unwrap(), artifact);
    assert_eq!(cached.manifest_signer_key_id(), CATALOG_KEY_ID);
    assert_eq!(cached.catalog_generation(), 1);
    std::fs::remove_file(cached.path()).unwrap();
    std::fs::write(cached.path(), b"modified bytes!!").unwrap();
    assert_eq!(
        cache.verify_cached(&verified),
        Err(ImageCacheErrorV1::CacheCorrupt)
    );
    assert_eq!(
        cache.remove_corrupt_for_approved_redownload(&verified, &DenyDownload),
        Err(ImageCacheErrorV1::ApprovalDenied)
    );
    let corrupt_path = cached.path().to_path_buf();
    cache
        .remove_corrupt_for_approved_redownload(&verified, &approval)
        .unwrap();
    assert!(!corrupt_path.exists());
    let mut replacement = Cursor::new(artifact);
    let repaired = cache
        .install_approved_reader(&verified, &approval, &mut replacement)
        .unwrap();
    assert_eq!(std::fs::read(repaired.path()).unwrap(), artifact);

    let external = root.path().join("outside-cache");
    std::fs::write(&external, artifact).unwrap();
    std::fs::remove_file(repaired.path()).unwrap();
    std::os::unix::fs::symlink(&external, repaired.path()).unwrap();
    assert_eq!(
        cache.verify_cached(&verified),
        Err(ImageCacheErrorV1::CacheCorrupt)
    );
    assert_eq!(
        cache.remove_corrupt_for_approved_redownload(&verified, &DenyDownload),
        Err(ImageCacheErrorV1::ApprovalDenied)
    );
    cache
        .remove_corrupt_for_approved_redownload(&verified, &approval)
        .unwrap();
    assert!(external.exists(), "cache repair removes only the symlink");
}

#[cfg(unix)]
#[path = "tests/overlay.rs"]
mod overlay;

#[cfg(unix)]
struct AllowExact {
    manifest: String,
    image: String,
}

#[cfg(unix)]
impl ComputerImageDownloadApprovalV1 for AllowExact {
    fn authorize_download(&self, manifest: &str, image: &str) -> Result<(), ImageCacheErrorV1> {
        if manifest == self.manifest && image == self.image {
            Ok(())
        } else {
            Err(ImageCacheErrorV1::ApprovalDenied)
        }
    }
}
