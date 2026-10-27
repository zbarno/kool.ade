use crate::execution::docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus};

use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
};

fn engine_state(status: &DockerEnvironmentStatus) -> (State, Code) {
    match status.state {
        DockerEnvironmentState::NotInstalled => (State::Missing, Code::DockerMissing),
        DockerEnvironmentState::NotRunning | DockerEnvironmentState::DesktopVmRequired => {
            (State::Missing, Code::DockerStopped)
        }
        DockerEnvironmentState::PermissionDenied => (State::Failed, Code::DockerPermissionDenied),
        DockerEnvironmentState::RootfulLinuxRejected => {
            (State::Unsupported, Code::DockerRootfulRejected)
        }
        DockerEnvironmentState::WindowsContainersRequired => {
            (State::Unsupported, Code::DockerWindowsContainers)
        }
        DockerEnvironmentState::InsufficientDisk => (State::Failed, Code::DockerDiskUnavailable),
        DockerEnvironmentState::LimitsUnavailable => (State::Failed, Code::DockerLimitsUnavailable),
        DockerEnvironmentState::CgroupsUnavailable => {
            (State::Unsupported, Code::DockerCgroupsUnavailable)
        }
        DockerEnvironmentState::SeccompUnavailable => {
            (State::Failed, Code::DockerSeccompUnavailable)
        }
        DockerEnvironmentState::UnsupportedApi => (State::Unsupported, Code::DockerApiUnsupported),
        DockerEnvironmentState::UnsupportedOs => (State::Unsupported, Code::DockerOsUnsupported),
        DockerEnvironmentState::UnsupportedArchitecture => {
            (State::Unsupported, Code::DockerArchitectureUnsupported)
        }
        DockerEnvironmentState::WorkspaceUnavailable => {
            (State::Failed, Code::DockerWorkspaceUnavailable)
        }
        DockerEnvironmentState::ProfileUnsupported => {
            (State::Unsupported, Code::DockerProfileUnsupported)
        }
        DockerEnvironmentState::UntrustedEndpoint => (State::Failed, Code::DockerEndpointUntrusted),
        DockerEnvironmentState::CapabilitiesIncompatible => {
            (State::Unknown, Code::DockerCapabilityUnknown)
        }
        DockerEnvironmentState::MissingApprovedImage | DockerEnvironmentState::Ready
            if status.engine_ready =>
        {
            (State::Ready, Code::DockerReady)
        }
        DockerEnvironmentState::ApprovedImageUncheckable
        | DockerEnvironmentState::ApprovedImageDigestMismatch
        | DockerEnvironmentState::ApprovedImageArchitectureMismatch
        | DockerEnvironmentState::MissingApprovedImage
        | DockerEnvironmentState::Ready => (State::Unknown, Code::DockerCapabilityUnknown),
    }
}

pub(super) fn push(
    result: &mut Vec<Evidence>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    generation: u64,
    now: u64,
) {
    let no_subject = Subject::None;
    let add = |capability, state, code| {
        evidence(
            id(Domain::Docker, host, capability, no_subject.clone()),
            state,
            code,
            Origin::DockerDiscovery,
            generation,
            None,
            now,
        )
    };
    let (state, code) = engine_state(docker);
    result.push(add(Capability::DockerEngine, state, code));
    let cgroups = match docker.cgroups_supported {
        Some(true) => (State::Ready, Code::DockerCgroupsSupported),
        Some(false) => (State::Unsupported, Code::DockerCgroupsUnavailable),
        None => (State::Unknown, Code::DockerCapabilityUnknown),
    };
    result.push(add(Capability::DockerCgroups, cgroups.0, cgroups.1));
    let endpoint = if docker.state == DockerEnvironmentState::UntrustedEndpoint {
        (State::Failed, Code::DockerEndpointUntrusted)
    } else if docker.engine_ready {
        (State::Ready, Code::DockerReady)
    } else if matches!(
        docker.state,
        DockerEnvironmentState::NotInstalled
            | DockerEnvironmentState::NotRunning
            | DockerEnvironmentState::DesktopVmRequired
    ) {
        (State::Missing, Code::DockerStopped)
    } else {
        (State::Unknown, Code::DockerCapabilityUnknown)
    };
    let mut endpoint_evidence = add(Capability::DockerEndpoint, endpoint.0, endpoint.1);
    if let Some(fingerprint) = docker.engine_identity_fingerprint() {
        endpoint_evidence.observation = Some(
            crate::app::setup_attention::catalog::ProbeObservationV1::DockerEngineIdentity {
                fingerprint,
            },
        );
    }
    result.push(endpoint_evidence);
    let rootless = if host != HostKind::Linux {
        (State::NotApplicable, Code::DockerCapabilityUnknown)
    } else {
        match docker.rootless {
            Some(true) => (State::Ready, Code::DockerReady),
            Some(false) if docker.state == DockerEnvironmentState::RootfulLinuxRejected => {
                (State::Missing, Code::DockerRootfulRejected)
            }
            Some(false) => (State::Missing, Code::DockerRootfulRejected),
            None => (State::Unknown, Code::DockerCapabilityUnknown),
        }
    };
    result.push(add(Capability::DockerRootless, rootless.0, rootless.1));
    let desktop_vm = if !matches!(host, HostKind::MacOs | HostKind::Windows) {
        (State::NotApplicable, Code::DockerCapabilityUnknown)
    } else if docker.state == DockerEnvironmentState::DesktopVmRequired {
        (State::Missing, Code::DockerStopped)
    } else if docker.desktop_vm && docker.engine_ready {
        (State::Ready, Code::DockerDesktopVmDetected)
    } else if docker.state == DockerEnvironmentState::NotInstalled {
        (State::Missing, Code::DockerMissing)
    } else {
        (State::Unknown, Code::DockerCapabilityUnknown)
    };
    result.push(add(Capability::DockerDesktopVm, desktop_vm.0, desktop_vm.1));
    let capacity = [docker.cpu_limits, docker.memory_limits, docker.pids_limit];
    let desktop_host = matches!(host, HostKind::MacOs | HostKind::Windows) || docker.desktop_vm;
    let desktop_capacity_verified = !desktop_host
        || (docker.desktop_vm_capacity_verified
            && docker.workspace_staging_available
            && docker.disk_available_bytes.is_some());
    let (state, code) = match docker.state {
        DockerEnvironmentState::InsufficientDisk => (State::Failed, Code::DockerDiskUnavailable),
        DockerEnvironmentState::LimitsUnavailable => (State::Failed, Code::DockerLimitsUnavailable),
        _ if !desktop_capacity_verified => (State::Unknown, Code::DockerCapacityUnavailable),
        _ if capacity.iter().all(|item| *item == Some(true)) => (State::Ready, Code::DockerReady),
        _ if capacity.contains(&Some(false)) => (State::Failed, Code::DockerCapacityUnavailable),
        _ => (State::Unknown, Code::DockerCapacityUnavailable),
    };
    result.push(add(Capability::DockerCapacity, state, code));
    result.push(add(
        Capability::DockerSecurity,
        if desktop_host && docker.desktop_vm && docker.desktop_transport_verified {
            State::Ready
        } else if desktop_host {
            State::Unknown
        } else if docker.state == DockerEnvironmentState::SeccompUnavailable {
            State::Failed
        } else if docker.engine_ready {
            State::Ready
        } else {
            State::Unknown
        },
        if desktop_host && docker.desktop_vm && docker.desktop_transport_verified {
            Code::DockerReady
        } else if desktop_host {
            Code::DockerDesktopTrustUnverified
        } else if docker.state == DockerEnvironmentState::SeccompUnavailable {
            Code::DockerSeccompUnavailable
        } else if docker.engine_ready {
            Code::DockerReady
        } else {
            Code::DockerCapabilityUnknown
        },
    ));
}
