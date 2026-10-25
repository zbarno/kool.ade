use crate::execution::docker::images::{ImageLock, RuntimeProfile};

use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    ImageArchitectureV1 as Architecture, ImageProfileV1 as Profile,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
};
use crate::execution::docker::environment::DockerEnvironmentStatus;

const PROFILES: [(Profile, RuntimeProfile); 5] = [
    (Profile::General, RuntimeProfile::General),
    (Profile::Rust, RuntimeProfile::Rust),
    (Profile::Node, RuntimeProfile::Node),
    (Profile::Python, RuntimeProfile::Python),
    (Profile::Go, RuntimeProfile::Go),
];

pub(super) fn push(
    result: &mut Vec<Evidence>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    generation: u64,
    now: u64,
) {
    let architecture = docker
        .architecture
        .as_deref()
        .map(Architecture::from_platform_arch)
        .unwrap_or(Architecture::Unknown);
    let lock = ImageLock::bundled().ok();
    for (profile, runtime_profile) in PROFILES {
        let subject = Subject::Image {
            profile,
            architecture,
        };
        let desktop_untrusted =
            docker.desktop_vm || matches!(host, HostKind::MacOs | HostKind::Windows);
        let image_failure = match docker.state {
            crate::execution::docker::environment::DockerEnvironmentState::ApprovedImageUncheckable => {
                Some((State::Unknown, Code::DockerImageUncheckable))
            }
            crate::execution::docker::environment::DockerEnvironmentState::ApprovedImageDigestMismatch => {
                Some((State::Failed, Code::DockerImageDigestMismatch))
            }
            crate::execution::docker::environment::DockerEnvironmentState::ApprovedImageArchitectureMismatch => {
                Some((State::Unsupported, Code::DockerImageArchitectureMismatch))
            }
            _ => None,
        };
        let (state, code) = if let Some(failure) = image_failure {
            failure
        } else if !docker.engine_ready || desktop_untrusted {
            (State::Blocked, Code::ParentPrerequisiteUnavailable)
        } else if let Some(arch) = architecture.image_name() {
            match lock
                .as_ref()
                .and_then(|lock| lock.select(runtime_profile, arch).ok())
            {
                Some(image) if docker.available_images.contains(&image.reference) => {
                    (State::Ready, Code::ImagePresent)
                }
                Some(_) => (State::Missing, Code::ImageMissing),
                None => (State::Unknown, Code::ImageUncheckable),
            }
        } else {
            (State::Unknown, Code::ImageUncheckable)
        };
        result.push(evidence(
            id(Domain::Image, host, Capability::ApprovedImage, subject),
            state,
            code,
            Origin::ImageInventory,
            generation,
            None,
            now,
        ));
    }
}
