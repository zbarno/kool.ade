use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    ImageArchitectureV1 as Architecture, ImageProfileV1 as Profile,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence, ProbeObservationV1 as Observation,
};
use crate::execution::docker::{
    environment::DockerEnvironmentStatus,
    images::{ImageLock, RuntimeProfile},
};

use super::{evidence, id};

pub(super) fn push(
    result: &mut Vec<Evidence>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    generation: u64,
    now: u64,
) {
    if host != HostKind::Linux
        || !docker.engine_ready
        || docker.desktop_vm
        || docker.rootless != Some(true)
    {
        return;
    }
    let Some(architecture_name) = docker.architecture.as_deref() else {
        return;
    };
    let Ok(lock) = ImageLock::bundled() else {
        return;
    };
    let Ok(image) = lock.select(RuntimeProfile::General, architecture_name) else {
        return;
    };
    if !docker.available_images.contains(&image.reference) {
        return;
    }
    let endpoint_identity = DockerEnvironmentStatus::current_endpoint_identity_fingerprint();
    let Some(receipt) = crate::persistence::linux_rootless_smoke_receipt::matching_current(
        endpoint_identity.as_deref(),
        docker.daemon_identity_fingerprint.as_deref(),
        docker.daemon_version.as_deref(),
        Some(&image),
    ) else {
        return;
    };
    let architecture = Architecture::from_platform_arch(architecture_name);
    let subject = Subject::Image {
        profile: Profile::General,
        architecture,
    };
    let mut item = evidence(
        id(
            Domain::Docker,
            host,
            Capability::DockerRootlessSmoke,
            subject,
        ),
        State::Ready,
        Code::ApprovedImageSmokeSucceeded,
        Origin::ApprovedImageSmokeTest,
        generation,
        None,
        now,
    );
    item.observation = Some(Observation::DockerRootlessSmoke {
        endpoint_fingerprint: receipt.endpoint_fingerprint,
        daemon_fingerprint: receipt.daemon_fingerprint,
        daemon_version: receipt.daemon_version,
        image_reference: receipt.image_reference,
        image_digest: receipt.image_digest,
        image_platform: receipt.image_platform,
        completed_at_unix_ms: receipt.completed_at_unix_ms,
    });
    result.push(item);
}
