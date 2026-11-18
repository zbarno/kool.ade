use std::fs;

use ed25519_dalek::SigningKey;
use koolade::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageStatusV1, verify_guest_image_catalog,
};

use super::{
    CandidateArchitecture, build_signed_bundle,
    candidate::{self, Candidate},
    previous::PreviousCatalog,
    tests::{dummy_candidate, synthetic_trust, write_candidate},
};

#[test]
fn candidate_architecture_selects_its_exact_sidecar_and_artifact_names() {
    for architecture in [
        CandidateArchitecture::X86_64,
        CandidateArchitecture::Aarch64,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = "a".repeat(40);
        let candidate_root = temp.path().join("candidate");
        let (_, _, agent) = write_candidate(&candidate_root, &source, architecture);
        let companion = temp.path().join("agent");
        fs::write(&companion, agent).unwrap();
        let attestations = temp.path().join("attestations.json");
        fs::write(&attestations, "[{},{}]").unwrap();

        let candidate = candidate::read_candidate(
            &candidate_root,
            &companion,
            &source,
            &attestations,
            architecture,
        )
        .unwrap();
        assert_eq!(
            candidate.image.architecture,
            architecture.manifest_architecture()
        );
        assert_eq!(candidate.image.artifact_name, architecture.image_name());
    }
}

#[test]
fn promoting_aarch64_preserves_the_approved_x86_64_image() {
    let signer = SigningKey::from_bytes(&[71; 32]);
    let trust = synthetic_trust(&signer);
    let mut x86 = dummy_candidate().image;
    x86.image_digest = format!("sha256:{}", "1".repeat(64));
    let x86_digest = x86.image_digest.clone();
    let mut arm = dummy_candidate().image;
    arm.architecture = ComputerGuestArchitectureV1::Aarch64;
    arm.artifact_name = CandidateArchitecture::Aarch64.image_name().into();
    arm.image_digest = format!("sha256:{}", "2".repeat(64));
    let arm_digest = arm.image_digest.clone();

    let (manifest, lock) = build_signed_bundle(
        Candidate { image: arm },
        PreviousCatalog {
            generation: 20,
            images: vec![x86],
        },
        21,
        1_000_000,
        &signer,
        &trust,
    )
    .unwrap();
    let verified =
        verify_guest_image_catalog(&serde_json::to_vec(&manifest).unwrap(), &trust, 1_000_000)
            .unwrap();

    let approved = verified.approved_images().collect::<Vec<_>>();
    assert_eq!(approved.len(), 2);
    assert!(
        approved
            .iter()
            .any(|image| image.image_digest == x86_digest)
    );
    assert!(
        approved
            .iter()
            .any(|image| image.image_digest == arm_digest)
    );
    assert_eq!(lock.images.len(), 2);
    assert!(lock.images.contains(&x86_digest));
    assert!(lock.images.contains(&arm_digest));

    let revoked_x86 = approved
        .iter()
        .find(|image| image.architecture == ComputerGuestArchitectureV1::X86_64);
    assert_eq!(revoked_x86.unwrap().status, GuestImageStatusV1::Approved);
}
