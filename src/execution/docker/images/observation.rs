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
        .any(|digest| digest == &selected.reference)
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
