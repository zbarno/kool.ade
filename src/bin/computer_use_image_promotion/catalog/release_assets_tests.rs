use std::{fs, path::Path};

use ed25519_dalek::SigningKey;
use koolade::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1, GuestImageStatusV1,
    verify_guest_image_catalog,
};

use super::super::{
    CandidateArchitecture, build_signed_bundle, candidate::Candidate, files::sha256_bytes,
    previous::PreviousCatalog,
};
use super::{attestation_name, stage_promotion_assets, verify_approved_assets};

fn entry(
    architecture: ComputerGuestArchitectureV1,
    image: &[u8],
    sbom: &[u8],
    attestations: &[u8],
) -> GuestImageEntryV1 {
    GuestImageEntryV1 {
        architecture,
        image_digest: sha256_bytes(image),
        image_size_bytes: 2 * 1024 * 1024 * 1024,
        artifact_size_bytes: image.len() as u64,
        artifact_name: architecture.artifact_name().to_owned(),
        guest_release: "debian-bookworm-fixture".into(),
        source_revision: "a".repeat(40),
        build_recipe_sha256: sha256_bytes(b"recipe"),
        sbom_sha256: sha256_bytes(sbom),
        build_attestation_sha256: sha256_bytes(attestations),
        companion_protocol_version: 1,
        companion_sha256: sha256_bytes(b"companion"),
        status: GuestImageStatusV1::Approved,
    }
}

fn write_release_asset(directory: &Path, image: &GuestImageEntryV1, bytes: &[u8], sbom: &[u8]) {
    fs::write(directory.join(&image.artifact_name), bytes).unwrap();
    fs::write(directory.join(sbom_name(&image.artifact_name)), sbom).unwrap();
}

fn sbom_name(image_name: &str) -> String {
    format!(
        "{}.sbom.cdx.json",
        image_name.strip_suffix(".qcow2").unwrap()
    )
}

fn trust(signer: &SigningKey) -> GuestImageManifestTrustV1 {
    GuestImageManifestTrustV1::from_application_pinned_keys(
        [(super::super::SIGNER_KEY_ID.into(), signer.verifying_key())],
        1,
    )
    .unwrap()
}

fn candidate(image: GuestImageEntryV1) -> Candidate {
    Candidate { image }
}

#[test]
fn promotion_carries_forward_and_verifies_each_architecture_asset() {
    // Production boundary: the workflow publishes the exact staged archive
    // files consumed by the runtime catalog verifier. This test proves that
    // preserved release assets remain digest-bound across architectures; it
    // needs no network, QEMU, or VM.
    let root = tempfile::tempdir().unwrap();
    let candidate_dir = root.path().join("candidate");
    let previous_release = root.path().join("previous-release");
    let output = root.path().join("promotion");
    fs::create_dir_all(&candidate_dir).unwrap();
    fs::create_dir_all(&previous_release).unwrap();
    fs::create_dir_all(&output).unwrap();

    let x86_image = b"previous x86 image";
    let x86_sbom = b"previous x86 sbom";
    let x86_attestations = b"previous x86 attestations";
    let x86 = entry(
        ComputerGuestArchitectureV1::X86_64,
        x86_image,
        x86_sbom,
        x86_attestations,
    );
    write_release_asset(&previous_release, &x86, x86_image, x86_sbom);
    // Older releases use this unqualified name for the x86 attestation.
    fs::write(
        previous_release.join("verified-attestations.json"),
        x86_attestations,
    )
    .unwrap();

    let signer = SigningKey::from_bytes(&[71; 32]);
    let trust = trust(&signer);
    let (previous_manifest, previous_lock) = build_signed_bundle(
        candidate(x86.clone()),
        PreviousCatalog {
            generation: 0,
            images: vec![],
        },
        20,
        1_000_000,
        &signer,
        &trust,
    )
    .unwrap();
    fs::write(
        previous_release.join("catalog-v1.json"),
        serde_json::to_vec_pretty(&previous_manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        previous_release.join("images.lock.json"),
        serde_json::to_vec_pretty(&previous_lock).unwrap(),
    )
    .unwrap();

    let arm_image = b"candidate arm64 image";
    let arm_sbom = b"candidate arm64 sbom";
    let arm_attestations = b"candidate arm64 attestations";
    let arm = entry(
        ComputerGuestArchitectureV1::Aarch64,
        arm_image,
        arm_sbom,
        arm_attestations,
    );
    write_release_asset(&candidate_dir, &arm, arm_image, arm_sbom);
    let arm_attestation_path = root.path().join("arm-attestations.json");
    fs::write(&arm_attestation_path, arm_attestations).unwrap();

    let previous = PreviousCatalog {
        generation: 20,
        images: previous_manifest.images.clone(),
    };
    stage_promotion_assets(
        &candidate_dir,
        &arm_attestation_path,
        &previous_release,
        &output,
        &previous,
        &arm,
        &trust,
    )
    .unwrap();
    let (manifest, _lock) = build_signed_bundle(
        candidate(arm.clone()),
        previous,
        21,
        1_000_000,
        &signer,
        &trust,
    )
    .unwrap();
    let verified =
        verify_guest_image_catalog(&serde_json::to_vec(&manifest).unwrap(), &trust, 1_000_000)
            .unwrap();

    verify_approved_assets(&verified, &output).unwrap();
    assert!(output.join(&x86.artifact_name).exists());
    assert!(output.join(&arm.artifact_name).exists());
    assert!(
        output
            .join(attestation_name(CandidateArchitecture::X86_64))
            .exists()
    );
    assert!(
        output
            .join(attestation_name(CandidateArchitecture::Aarch64))
            .exists()
    );

    fs::write(
        output.join(attestation_name(CandidateArchitecture::X86_64)),
        b"changed attestations",
    )
    .unwrap();
    assert!(verify_approved_assets(&verified, &output).is_err());
}
