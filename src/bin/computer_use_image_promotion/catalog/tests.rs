use std::{fs, path::Path};

use ed25519_dalek::SigningKey;
use koolade::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1, GuestImageStatusV1,
    verify_guest_image_catalog,
};
use sha2::{Digest, Sha256};

use super::{
    build_signed_bundle,
    candidate::{self, Candidate},
    files::sha256_bytes,
    previous::PreviousCatalog,
};

fn write_candidate(root: &Path, source: &str) -> (String, String, Vec<u8>) {
    fs::create_dir_all(root).unwrap();
    let image = b"synthetic-qcow2-candidate";
    let agent = b"synthetic-guest-agent";
    let sbom = br#"{"bomFormat":"CycloneDX","specVersion":"1.6","version":1}"#;
    let image_digest = format!("sha256:{:x}", Sha256::digest(image));
    let agent_digest = format!("{:x}", Sha256::digest(agent));
    let sbom_digest = format!("{:x}", Sha256::digest(sbom));
    fs::write(
        root.join(format!("{}.qcow2", candidate::IMAGE_BASENAME)),
        image,
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.sha256", candidate::IMAGE_BASENAME)),
        format!(
            "{:}  {}.qcow2\n",
            &image_digest[7..],
            candidate::IMAGE_BASENAME
        ),
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.agent.sha256", candidate::IMAGE_BASENAME)),
        format!("{agent_digest}  {}.agent\n", candidate::IMAGE_BASENAME),
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.qemu-info.json", candidate::IMAGE_BASENAME)),
        r#"{"virtual-size":2147483648}"#,
    )
    .unwrap();
    let recipe = b"synthetic recipe components";
    fs::write(
        root.join(format!(
            "{}.recipe-components.sha256",
            candidate::IMAGE_BASENAME
        )),
        recipe,
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.recipe.sha256", candidate::IMAGE_BASENAME)),
        format!("{}\n", sha256_bytes(recipe)),
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.sbom.cdx.json", candidate::IMAGE_BASENAME)),
        sbom,
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.sbom.sha256", candidate::IMAGE_BASENAME)),
        format!(
            "{sbom_digest}  {}.sbom.cdx.json\n",
            candidate::IMAGE_BASENAME
        ),
    )
    .unwrap();
    fs::write(
        root.join(format!("{}.source-revision", candidate::IMAGE_BASENAME)),
        format!("{source}\n"),
    )
    .unwrap();
    (
        image_digest,
        format!("sha256:{agent_digest}"),
        agent.to_vec(),
    )
}

fn synthetic_trust(signer: &SigningKey) -> GuestImageManifestTrustV1 {
    GuestImageManifestTrustV1::from_application_pinned_keys(
        [(super::SIGNER_KEY_ID.into(), signer.verifying_key())],
        1,
    )
    .unwrap()
}

#[test]
fn signing_binds_candidate_sidecars_companion_attestations_and_lock() {
    // Production boundary: generated catalog and lock are consumed by the
    // app-pinned verifier. This unit test proves their cryptographic and digest
    // relationships without network, QEMU, or a VM.
    let temp = tempfile::tempdir().unwrap();
    let source = "a".repeat(40);
    let candidate_root = temp.path().join("candidate");
    let (image_digest, agent_digest, agent) = write_candidate(&candidate_root, &source);
    let companion = temp.path().join("agent");
    fs::write(&companion, agent).unwrap();
    let attestations = temp.path().join("attestations.json");
    fs::write(&attestations, "[{},{}]").unwrap();
    let candidate =
        candidate::read_candidate(&candidate_root, &companion, &source, &attestations).unwrap();
    let signer = SigningKey::from_bytes(&[71; 32]);
    let trust = synthetic_trust(&signer);
    let (manifest, lock) = build_signed_bundle(
        candidate,
        PreviousCatalog {
            generation: 0,
            images: vec![],
        },
        1,
        1_000_000,
        &signer,
        &trust,
    )
    .unwrap();
    let verified =
        verify_guest_image_catalog(&serde_json::to_vec(&manifest).unwrap(), &trust, 1_000_000)
            .unwrap();
    assert_eq!(verified.digest(), lock.manifest_sha256.as_deref().unwrap());
    assert_eq!(lock.images, [image_digest]);
    let approved = verified.approved_images().next().unwrap();
    assert_eq!(approved.companion_sha256, agent_digest);
    assert_eq!(approved.source_revision, source);
    assert_eq!(approved.status, GuestImageStatusV1::Approved);
    assert!(
        manifest
            .catalog_version
            .starts_with("computer-use-images-v1-1-")
    );
}

#[test]
fn signing_rejects_a_key_that_is_not_in_the_supplied_trust_store() {
    let signer = SigningKey::from_bytes(&[71; 32]);
    let wrong_signer = SigningKey::from_bytes(&[72; 32]);
    assert!(
        build_signed_bundle(
            dummy_candidate(),
            PreviousCatalog {
                generation: 0,
                images: vec![]
            },
            1,
            1_000_000,
            &wrong_signer,
            &synthetic_trust(&signer),
        )
        .is_err()
    );
}

#[test]
fn promotion_rejects_a_changed_companion_sidecar() {
    let temp = tempfile::tempdir().unwrap();
    let source = "a".repeat(40);
    let candidate_root = temp.path().join("candidate");
    let (_, _, agent) = write_candidate(&candidate_root, &source);
    let companion = temp.path().join("agent");
    fs::write(&companion, agent).unwrap();
    fs::write(
        candidate_root.join(format!("{}.agent.sha256", candidate::IMAGE_BASENAME)),
        format!("{}  {}.agent\n", "0".repeat(64), candidate::IMAGE_BASENAME),
    )
    .unwrap();
    let attestations = temp.path().join("attestations.json");
    fs::write(&attestations, "[{},{}]").unwrap();
    assert!(
        candidate::read_candidate(&candidate_root, &companion, &source, &attestations).is_err()
    );
}

fn dummy_candidate() -> Candidate {
    let digest = format!("sha256:{}", "c".repeat(64));
    Candidate {
        image: GuestImageEntryV1 {
            architecture: ComputerGuestArchitectureV1::X86_64,
            image_digest: digest.clone(),
            image_size_bytes: 2 * 1024 * 1024 * 1024,
            artifact_size_bytes: 8,
            artifact_name: candidate::IMAGE_NAME.into(),
            guest_release: "debian-bookworm-test".into(),
            source_revision: "b".repeat(40),
            build_recipe_sha256: digest.clone(),
            sbom_sha256: digest.clone(),
            build_attestation_sha256: digest.clone(),
            companion_protocol_version: 1,
            companion_sha256: digest,
            status: GuestImageStatusV1::Approved,
        },
    }
}
