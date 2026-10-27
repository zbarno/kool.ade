use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    MachineSnapshot, PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1, ProbeObservationV1 as Observation,
};

pub(in crate::persistence::setup_tasks) fn positive_domain_for(
    id: &str,
    snapshot: &MachineSnapshot,
) -> Option<String> {
    match id {
        "setup:linux:rootless:packages" if rootless_helpers_ready(snapshot) => {
            return Some("host:linux:rootless_helpers".to_owned());
        }
        "setup:linux:rootless:subids" if subordinate_ids_ready(snapshot) => {
            return Some("host:linux:subordinate_ids".to_owned());
        }
        "setup:linux:rootless:service" | "setup:docker:rootless" => {
            return rootless_smoke_ready(snapshot).then(|| "docker:rootless-smoke".to_owned());
        }
        _ => {}
    }
    if id == "setup:legacy:provider" {
        return snapshot
            .checked_domains
            .iter()
            .find(|domain| domain.starts_with("cli:"))
            .cloned();
    }
    if matches!(
        id,
        "setup:docker:image-unverified"
            | "setup:docker:image-digest"
            | "setup:docker:image-architecture"
            | "setup:images:general:unverified"
    ) {
        let architecture = match snapshot.capability_matrix.observed_host_architecture {
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64 => "amd64",
            crate::app::setup_attention::catalog::ImageArchitectureV1::Arm64 => "arm64",
            crate::app::setup_attention::catalog::ImageArchitectureV1::Unknown
            | crate::app::setup_attention::catalog::ImageArchitectureV1::Unsupported => {
                return None;
            }
        };
        let expected = format!("images:general:{architecture}");
        return snapshot
            .checked_domains
            .iter()
            .find(|domain| domain.as_str() == expected)
            .cloned();
    }
    if super::validation::desktop_docker_evidence(id, "docker:desktop")
        && snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    {
        return Some("docker:desktop".to_owned());
    }
    let domain = super::validation::expected_evidence_domain(id)?;
    snapshot
        .checked_domains
        .iter()
        .any(|checked| checked == &domain)
        .then_some(domain)
}

fn rootless_helpers_ready(snapshot: &MachineSnapshot) -> bool {
    current_linux_host_fact(snapshot, Capability::LinuxRootlessHelpers).is_some_and(|fact| {
        fact.code == Code::LinuxHostCheckReady
            && matches!(
                &fact.observation,
                Some(Observation::LinuxRootlessHelpers {
                    newuidmap: true,
                    newgidmap: true,
                    rootlesskit: true,
                    dockerd_rootless: true,
                    setuptool: true,
                    ..
                })
            )
    })
}

fn subordinate_ids_ready(snapshot: &MachineSnapshot) -> bool {
    let subuid = current_linux_host_fact(snapshot, Capability::LinuxSubuid);
    let subgid = current_linux_host_fact(snapshot, Capability::LinuxSubgid);
    [subuid, subgid].into_iter().all(|fact| {
        fact.is_some_and(|fact| {
            fact.code == Code::LinuxHostCheckReady
                && matches!(
                    &fact.observation,
                    Some(Observation::LinuxSubordinateIds { configured: true })
                )
        })
    })
}

fn current_linux_host_fact(
    snapshot: &MachineSnapshot,
    capability: Capability,
) -> Option<&ProbeEvidenceV1> {
    if snapshot.host != HostKind::Linux {
        return None;
    }
    let mut matches = snapshot.probe_evidence.iter().filter(|fact| {
        fact.prerequisite.domain == Domain::Host
            && fact.prerequisite.platform == HostKind::Linux
            && fact.prerequisite.capability == capability
            && fact.prerequisite.subject == Subject::None
            && fact.state == State::Ready
            && fact.origin == Origin::HostRuntime
            && fact.scan_generation == snapshot.scan_generation
            && fact.source_generation.is_none()
    });
    let fact = matches.next()?;
    matches.next().is_none().then_some(fact)
}

fn rootless_smoke_ready(snapshot: &MachineSnapshot) -> bool {
    use crate::app::setup_attention::catalog::{
        ImageArchitectureV1, ImageProfileV1, ProbeObservationV1 as Observation,
    };

    if snapshot.host != HostKind::Linux
        || !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:rootless-smoke")
    {
        return false;
    }
    snapshot.probe_evidence.iter().any(|fact| {
        if fact.prerequisite.domain != Domain::Docker
            || fact.prerequisite.platform != HostKind::Linux
            || fact.prerequisite.capability != Capability::DockerRootlessSmoke
            || fact.state != State::Ready
            || fact.code != Code::ApprovedImageSmokeSucceeded
            || fact.origin != Origin::ApprovedImageSmokeTest
            || fact.scan_generation != snapshot.scan_generation
            || fact.source_generation.is_some()
        {
            return false;
        }
        let architecture = match &fact.prerequisite.subject {
            Subject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Amd64,
            } => "amd64",
            Subject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Arm64,
            } => "arm64",
            _ => return false,
        };
        let Some(Observation::DockerRootlessSmoke {
            endpoint_fingerprint,
            daemon_fingerprint,
            daemon_version,
            image_reference,
            image_digest,
            image_platform,
            completed_at_unix_ms,
        }) = &fact.observation
        else {
            return false;
        };
        let Ok(lock) = crate::execution::docker::images::ImageLock::bundled() else {
            return false;
        };
        let Ok(image) = lock.select(
            crate::execution::docker::images::RuntimeProfile::General,
            architecture,
        ) else {
            return false;
        };
        valid_fingerprint(endpoint_fingerprint)
            && valid_fingerprint(daemon_fingerprint)
            && !daemon_version.is_empty()
            && daemon_version.len() <= 128
            && !daemon_version.bytes().any(|byte| byte.is_ascii_control())
            && image_reference == &image.reference
            && image_digest == &image.digest
            && image_platform == &image.platform
            && *completed_at_unix_ms > 0
            && *completed_at_unix_ms <= snapshot.checked_at_unix_ms
    })
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
