use super::{MachineSnapshotV1, current_ready, docker_ready, images};
use crate::app::setup_attention::catalog::{
    EvidenceState, ImageArchitectureV1, ImageProfileV1, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject,
};
use sha2::{Digest, Sha256};

pub(super) fn ready(
    snapshot: &MachineSnapshotV1,
    expected_architecture: ImageArchitectureV1,
) -> bool {
    use crate::app::setup_attention::catalog::ProbeObservationV1 as Observation;

    let Some(smoke) = snapshot.probe_evidence.iter().find(|item| {
        item.prerequisite.domain == Domain::Docker
            && item.prerequisite.capability == Capability::DockerRootlessSmoke
            && item.state == EvidenceState::Ready
            && item.scan_generation == snapshot.scan_generation
            && item.prerequisite.subject
                == (Subject::Image {
                    profile: ImageProfileV1::General,
                    architecture: expected_architecture,
                })
    }) else {
        return false;
    };
    let architecture_name = match expected_architecture {
        ImageArchitectureV1::Amd64 => "amd64",
        ImageArchitectureV1::Arm64 => "arm64",
        _ => return false,
    };
    let subject = Subject::Image {
        profile: ImageProfileV1::General,
        architecture: expected_architecture,
    };
    if !current_ready(
        snapshot,
        Domain::Docker,
        Capability::DockerRootlessSmoke,
        subject,
        false,
    ) {
        return false;
    }
    let Some(Observation::DockerRootlessSmoke {
        endpoint_fingerprint,
        daemon_fingerprint,
        image_reference,
        image_digest,
        image_platform,
        ..
    }) = &smoke.observation
    else {
        return false;
    };
    let Some(expected_engine_identity) =
        engine_identity_fingerprint(endpoint_fingerprint, daemon_fingerprint)
    else {
        return false;
    };
    let Ok(lock) = crate::execution::docker::images::ImageLock::bundled() else {
        return false;
    };
    let Ok(image) = lock.select(
        crate::execution::docker::images::RuntimeProfile::General,
        architecture_name,
    ) else {
        return false;
    };
    docker_ready(snapshot)
        && images::docker_engine_identity(snapshot) == Some(expected_engine_identity.as_str())
        && images::image_inventory_domain_ready(
            snapshot,
            ImageProfileV1::General,
            expected_architecture,
        )
        && image_reference == &image.reference
        && image_digest == &image.digest
        && image_platform == &image.platform
}

fn engine_identity_fingerprint(endpoint: &str, daemon: &str) -> Option<String> {
    if !images::is_lower_sha256(endpoint) || !images::is_lower_sha256(daemon) {
        return None;
    }
    let identity = serde_json::to_vec(&("koolade-engine-identity-v1", endpoint, daemon)).ok()?;
    Some(format!("{:x}", Sha256::digest(identity)))
}

pub(super) fn ready_for_any_architecture(snapshot: &MachineSnapshotV1) -> bool {
    [ImageArchitectureV1::Amd64, ImageArchitectureV1::Arm64]
        .into_iter()
        .any(|architecture| ready(snapshot, architecture))
}
