use std::collections::HashSet;

use super::{ImageLock, SelectedImage, failure};
use crate::execution::docker::discovery::{
    PrerequisiteCode, PrerequisiteFailure, parse_architecture,
};

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

/// Return only exact locked references observed on the daemon.
pub fn approved_available_images(lock: &ImageLock, observed: &[String]) -> Vec<String> {
    let observed: HashSet<&str> = observed.iter().map(String::as_str).collect();
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
