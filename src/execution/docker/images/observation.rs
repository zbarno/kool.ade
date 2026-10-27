use std::borrow::Cow;
use std::collections::HashSet;

use super::{ImageLock, SelectedImage, failure, is_digest};
use crate::execution::docker::discovery::{
    PrerequisiteCode, PrerequisiteFailure, parse_architecture,
};

pub fn verify_derived_image_observation(
    selected: &SelectedImage,
    content_identity: &str,
    base_digest: &str,
    observation: &super::ImageObservation,
) -> Result<(), PrerequisiteFailure> {
    let valid_digest = |value: &str| {
        value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    };
    if !valid_digest(&selected.digest)
        || selected.reference != selected.digest
        || !valid_digest(content_identity)
        || !valid_digest(base_digest)
        || observation.image_id != selected.digest
        || observation
            .labels
            .get("io.koolade.content")
            .map(String::as_str)
            != Some(content_identity)
        || observation
            .labels
            .get("io.koolade.base-digest")
            .map(String::as_str)
            != Some(base_digest)
    {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "Registered derived image identity does not match the inspected Docker image",
            false,
        ));
    }
    let expected_arch = selected.platform.strip_prefix("linux/").unwrap_or_default();
    if !observation.os.eq_ignore_ascii_case("linux")
        || parse_architecture(&observation.architecture) != Some(expected_arch)
    {
        return Err(failure(
            PrerequisiteCode::ImageArchitectureMismatch,
            "The derived image platform does not match its registered target architecture",
            false,
        ));
    }
    Ok(())
}

pub fn verify_image_observation(
    selected: &SelectedImage,
    observed_repo_digests: &[String],
    observed_os: &str,
    observed_arch: &str,
) -> Result<(), PrerequisiteFailure> {
    if !observed_repo_digests
        .iter()
        .any(|digest| matches_docker_reference(&selected.reference, digest))
    {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            &format!(
                "The local image does not have the exact approved repository digest; expected {}, observed {}",
                selected.reference,
                if observed_repo_digests.is_empty() {
                    "no repository digests".to_owned()
                } else {
                    observed_repo_digests.join(", ")
                }
            ),
            false,
        ));
    }
    let expected_arch = selected.platform.strip_prefix("linux/").unwrap_or_default();
    if !observed_os.eq_ignore_ascii_case("linux")
        || parse_architecture(observed_arch) != Some(expected_arch)
    {
        return Err(failure(
            PrerequisiteCode::ImageArchitectureMismatch,
            "The local image platform does not match the approved Linux architecture",
            false,
        ));
    }
    Ok(())
}

fn matches_docker_reference(expected: &str, observed: &str) -> bool {
    let Some((expected_repository, expected_digest)) = expected.rsplit_once('@') else {
        return false;
    };
    let Some((observed_repository, observed_digest)) = observed.rsplit_once('@') else {
        return false;
    };
    expected_digest == observed_digest
        && canonical_docker_repository(expected_repository)
            == canonical_docker_repository(observed_repository)
}

fn canonical_docker_repository(repository: &str) -> String {
    if let Some(repository) = repository
        .strip_prefix("docker.io/library/")
        .or_else(|| repository.strip_prefix("index.docker.io/library/"))
    {
        return format!("docker.io/library/{repository}");
    }
    if !repository_has_registry(repository) {
        return format!("docker.io/library/{repository}");
    }
    repository.to_owned()
}

fn repository_has_registry(repository: &str) -> bool {
    let first = repository.split('/').next().unwrap_or_default();
    first.contains('.') || first.contains(':') || first == "localhost"
}

/// Normalize a daemon-reported repository digest with the same repository
/// canonicalization `matches_docker_reference` uses, so set-based inventory
/// lookups accept engines that elide the default-registry prefix from
/// reported digests. References carrying an explicit registry, or values that
/// are not digest specifications, pass through unchanged.
fn canonical_repository_digest(reference: &str) -> Cow<'_, str> {
    let Some((repository, digest)) = reference.rsplit_once('@') else {
        return Cow::Borrowed(reference);
    };
    if !is_digest(digest) {
        return Cow::Borrowed(reference);
    }
    let canonical_repository = canonical_docker_repository(repository);
    if canonical_repository == repository {
        Cow::Borrowed(reference)
    } else {
        Cow::Owned(format!("{canonical_repository}@{digest}"))
    }
}

/// Return only exact locked references observed on the daemon.
pub fn approved_available_images(lock: &ImageLock, observed: &[String]) -> Vec<String> {
    let observed_canonical: Vec<String> = observed
        .iter()
        .map(|reference| canonical_repository_digest(reference).into_owned())
        .collect();
    let observed: HashSet<&str> = observed_canonical.iter().map(String::as_str).collect();
    let mut available: Vec<String> = lock
        .profiles
        .values()
        .flatten()
        .filter(|variant| observed.contains(variant.reference.as_str()))
        .map(|variant| variant.reference.clone())
        .collect();
    available.sort();
    available.dedup();
    available
}
