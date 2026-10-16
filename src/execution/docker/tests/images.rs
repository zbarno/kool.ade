use super::super::{
    discovery::PrerequisiteCode,
    images::{
        ImageLock, ImageObservation, RuntimeProfile, SelectedImage, approved_available_images,
        verify_derived_image_observation, verify_image_observation,
    },
};
use std::collections::BTreeMap;

#[test]
fn bundled_profiles_resolve_digest_only_architecture_specific_images() {
    let lock = ImageLock::bundled().unwrap();
    let image = lock.select(RuntimeProfile::Rust, "x86_64").unwrap();
    assert_eq!(image.platform, "linux/amd64");
    assert_eq!(image.runtime_user, "10001:10001");
    assert!(image.reference.ends_with(&image.digest));
    assert!(lock.select(RuntimeProfile::BrowserQa, "amd64").is_err());
    assert!(lock.select(RuntimeProfile::General, "s390x").is_err());
}

#[test]
fn image_manifest_rejects_unapproved_registry_and_profile_names() {
    let mut lock = ImageLock::bundled().unwrap();
    let image = lock
        .profiles
        .get_mut("general")
        .unwrap()
        .first_mut()
        .unwrap();
    let digest = image.digest.clone();
    image.reference = format!("attacker.invalid/repository@{digest}");
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ImageUnavailable
    );
    let mut lock = ImageLock::bundled().unwrap();
    lock.profiles.insert("arbitrary".into(), Vec::new());
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ProfileUnsupported
    );
}

#[test]
fn image_manifest_accepts_only_the_trusted_upstream_repository() {
    let mut lock = ImageLock::bundled().unwrap();
    let image = lock.profiles.get("general").unwrap().first().unwrap();
    let digest = image.digest.clone();
    let trusted_reference = image.reference.clone();
    lock.profiles
        .get_mut("general")
        .unwrap()
        .first_mut()
        .unwrap()
        .reference = trusted_reference;
    lock.validate().unwrap();

    lock.profiles
        .get_mut("general")
        .unwrap()
        .first_mut()
        .unwrap()
        .reference = format!("ghcr.io/other/koolade-executor@{digest}");
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ImageUnavailable
    );
}

#[test]
fn image_manifest_requires_profile_metadata_architectures_and_pinned_rust_version() {
    let mut lock = ImageLock::bundled().unwrap();
    lock.profiles
        .get_mut("rust")
        .unwrap()
        .first_mut()
        .unwrap()
        .tool_versions
        .insert("cargo".into(), "1.90.0".into());
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ImageUnavailable
    );

    let mut lock = ImageLock::bundled().unwrap();
    lock.profiles
        .get_mut("node")
        .unwrap()
        .first_mut()
        .unwrap()
        .tool_versions
        .remove("npm");
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ImageUnavailable
    );

    let mut lock = ImageLock::bundled().unwrap();
    lock.profiles.get_mut("general").unwrap().pop();
    assert_eq!(
        lock.validate().unwrap_err().code,
        PrerequisiteCode::ImageUnavailable
    );
}

#[test]
fn image_verification_rejects_tags_spoofed_digests_missing_images_and_wrong_platform() {
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    assert_eq!(
        verify_image_observation(
            &image,
            &["docker.io/library/debian:bookworm-slim".into()],
            "linux",
            "amd64"
        )
        .unwrap_err()
        .code,
        PrerequisiteCode::ImageDigestMismatch
    );
    assert_eq!(
        verify_image_observation(&image, &[], "linux", "amd64")
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
    assert_eq!(
        verify_image_observation(
            &image,
            std::slice::from_ref(&image.reference),
            "linux",
            "arm64"
        )
        .unwrap_err()
        .code,
        PrerequisiteCode::ImageArchitectureMismatch
    );
    assert_eq!(
        verify_image_observation(
            &image,
            std::slice::from_ref(&image.reference),
            "windows",
            "amd64"
        )
        .unwrap_err()
        .code,
        PrerequisiteCode::ImageArchitectureMismatch
    );
    verify_image_observation(
        &image,
        std::slice::from_ref(&image.reference),
        "linux",
        "x86_64",
    )
    .unwrap();
}

#[test]
fn image_verification_accepts_docker_hub_library_alias_only_for_the_same_digest() {
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::Node, "amd64")
        .unwrap();
    let docker_hub_short_name = image.reference.replace("docker.io/library/", "");
    verify_image_observation(&image, &[docker_hub_short_name], "linux", "amd64").unwrap();

    let wrong_digest = format!("node@sha256:{}", "0".repeat(64));
    assert_eq!(
        verify_image_observation(&image, &[wrong_digest], "linux", "amd64")
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
    let wrong_repository = image
        .reference
        .replace("docker.io/library/node", "attacker.invalid/library/node");
    assert_eq!(
        verify_image_observation(&image, &[wrong_repository], "linux", "amd64")
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
}

#[test]
fn derived_image_verification_requires_registered_content_and_daemon_identity() {
    let image = SelectedImage {
        profile: RuntimeProfile::General,
        platform: "linux/amd64".into(),
        reference: format!("sha256:{}", "1".repeat(64)),
        digest: format!("sha256:{}", "1".repeat(64)),
        runtime_user: "10001:10001".into(),
        tool_versions: BTreeMap::new(),
    };
    let content = format!("sha256:{}", "2".repeat(64));
    let base = format!("sha256:{}", "3".repeat(64));
    let observation = ImageObservation {
        image_id: image.digest.clone(),
        repo_digests: Vec::new(),
        os: "linux".into(),
        architecture: "amd64".into(),
        configured_user: "10001:10001".into(),
        labels: BTreeMap::from([
            ("io.koolade.content".into(), content.clone()),
            ("io.koolade.base-digest".into(), base.clone()),
        ]),
    };
    verify_derived_image_observation(&image, &content, &base, &observation).unwrap();

    let mut tampered = observation.clone();
    tampered.image_id = format!("sha256:{}", "4".repeat(64));
    assert_eq!(
        verify_derived_image_observation(&image, &content, &base, &tampered)
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
    let mut unregistered = observation;
    unregistered.labels.remove("io.koolade.content");
    assert_eq!(
        verify_derived_image_observation(&image, &content, &base, &unregistered)
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
}

#[test]
fn engine_inventory_exposes_only_exact_locked_image_references() {
    let lock = ImageLock::bundled().unwrap();
    let approved = lock
        .select(RuntimeProfile::General, "amd64")
        .unwrap()
        .reference;
    let available = approved_available_images(
        &lock,
        &[
            approved.clone(),
            "docker.io/library/debian:bookworm-slim".into(),
            "attacker.invalid/tool@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        ],
    );

    assert_eq!(available, vec![approved]);
}
