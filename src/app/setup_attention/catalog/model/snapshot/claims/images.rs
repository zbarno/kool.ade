use super::{MachineSnapshotV1, current_ready, docker_ready, rootless_smoke};
use crate::app::setup_attention::catalog::{
    EvidenceState, ImageArchitectureV1, ImageProfileV1, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject,
};

pub(super) fn image_domain_ready(
    snapshot: &MachineSnapshotV1,
    profile: &str,
    architecture: &str,
    engine_identity: Option<&str>,
) -> bool {
    let profile = match profile {
        "general" => ImageProfileV1::General,
        "rust" => ImageProfileV1::Rust,
        "node" => ImageProfileV1::Node,
        "python" => ImageProfileV1::Python,
        "go" => ImageProfileV1::Go,
        _ => return false,
    };
    let architecture = match architecture {
        "amd64" => ImageArchitectureV1::Amd64,
        "arm64" => ImageArchitectureV1::Arm64,
        _ => return false,
    };
    let identity_matches =
        engine_identity.is_none_or(|expected| docker_engine_identity(snapshot) == Some(expected));
    image_inventory_domain_ready(snapshot, profile, architecture)
        && identity_matches
        && (profile != ImageProfileV1::General || rootless_smoke::ready(snapshot, architecture))
}

pub(super) fn image_inventory_domain_ready(
    snapshot: &MachineSnapshotV1,
    profile: ImageProfileV1,
    architecture: ImageArchitectureV1,
) -> bool {
    snapshot
        .capability_matrix
        .supported_runtime_architectures
        .contains(&architecture)
        && docker_ready(snapshot)
        && current_ready(
            snapshot,
            Domain::Image,
            Capability::ApprovedImage,
            Subject::Image {
                profile,
                architecture,
            },
            false,
        )
}

pub(super) fn docker_engine_identity(snapshot: &MachineSnapshotV1) -> Option<&str> {
    if !docker_ready(snapshot) {
        return None;
    }
    let evidence = snapshot.probe_evidence.iter().find(|item| {
        item.prerequisite.domain == Domain::Docker
            && item.prerequisite.capability == Capability::DockerEndpoint
            && item.prerequisite.subject == Subject::None
            && item.prerequisite.platform == snapshot.host
            && item.scan_generation == snapshot.scan_generation
            && item.source_generation.is_none()
            && item.origin == crate::app::setup_attention::catalog::EvidenceOrigin::DockerDiscovery
            && item.state == EvidenceState::Ready
            && item.code == crate::app::setup_attention::catalog::EvidenceCode::DockerReady
    })?;
    let Some(crate::app::setup_attention::catalog::ProbeObservationV1::DockerEngineIdentity {
        fingerprint,
    }) = evidence.observation.as_ref()
    else {
        return None;
    };
    is_lower_sha256(fingerprint).then_some(fingerprint)
}

pub(super) fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
