use crate::app::bootstrap_approval::ApprovedAction;
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{EngineDiscovery, EngineSnapshot, approval_fingerprint, failure},
    images::SelectedImage,
};

pub(super) fn matches_approval(
    engine: &EngineDiscovery,
    authorization: &ApprovedAction,
    snapshot: &EngineSnapshot,
    image: &SelectedImage,
) -> Result<bool, PrerequisiteFailure> {
    let fingerprint = approval_fingerprint(engine.endpoint(), snapshot, image).map_err(|_| {
        failure(
            PrerequisiteCode::ImageDigestMismatch,
            "The approved Engine fingerprint could not be verified",
            false,
        )
    })?;
    Ok(fingerprint == authorization.engine_fingerprint())
}

pub(super) fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn pull_cancelled() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Approved image preparation was cancelled",
        true,
    )
}
