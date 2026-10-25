use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxDistributionVersionV1 as DistributionVersion,
    LinuxRootlessMechanismV1 as RootlessMechanism, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
    ProbeObservationV1 as Observation, SupportClaim,
};
use crate::execution::docker::environment::DockerEnvironmentStatus;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

mod probe;
#[cfg(test)]
#[path = "host/tests.rs"]
mod tests;

pub(super) struct HostFacts {
    linux_distribution: State,
    linux_distribution_id: Option<Distribution>,
    linux_distribution_version: Option<DistributionVersion>,
    linux_distribution_support: Option<SupportClaim>,
    linux_uid_mappings: State,
    linux_service_manager: State,
    linux_package_manager: State,
    linux_rootless_mechanism: Option<RootlessMechanism>,
    windows_wsl2: State,
    virtualization: State,
    docker_desktop_vm_trust: State,
}

pub(super) fn probe(host: HostKind, docker: &DockerEnvironmentStatus) -> HostFacts {
    let linux = host == HostKind::Linux;
    let desktop = matches!(host, HostKind::MacOs | HostKind::Windows) || docker.desktop_vm;
    let distribution = linux.then(probe::linux_distribution).flatten();
    let linux_distribution_id = distribution.map(|(id, _)| id);
    let linux_distribution_version = distribution.and_then(|(_, version)| version);
    let linux_service_manager = if linux {
        probe::linux_service_manager()
    } else {
        State::NotApplicable
    };
    HostFacts {
        linux_distribution: if !linux {
            State::NotApplicable
        } else if linux_distribution_id.is_some() {
            State::Ready
        } else {
            State::Unknown
        },
        linux_distribution_id,
        linux_distribution_version,
        linux_distribution_support: distribution_support(
            host,
            linux_distribution_id,
            linux_distribution_version,
        ),
        linux_uid_mappings: if linux {
            probe::linux_uid_mappings()
        } else {
            State::NotApplicable
        },
        linux_service_manager,
        linux_package_manager: if linux {
            probe::linux_package_manager()
        } else {
            State::NotApplicable
        },
        linux_rootless_mechanism: rootless_mechanism(host, linux_service_manager),
        windows_wsl2: if host == HostKind::Windows {
            State::Unknown
        } else {
            State::NotApplicable
        },
        virtualization: if matches!(host, HostKind::MacOs | HostKind::Windows) {
            State::Unknown
        } else {
            State::NotApplicable
        },
        docker_desktop_vm_trust: if desktop {
            State::Unknown
        } else {
            State::NotApplicable
        },
    }
}

pub(super) fn push(
    result: &mut Vec<Evidence>,
    host: HostKind,
    generation: u64,
    now: u64,
    facts: &HostFacts,
) {
    let add = |capability, state, code| {
        evidence(
            id(Domain::Host, host, capability, Subject::None),
            state,
            code,
            Origin::HostRuntime,
            generation,
            None,
            now,
        )
    };
    result.push(add(
        Capability::HostFamily,
        State::Ready,
        Code::RuntimeTargetDetected,
    ));
    result.push(add(
        Capability::HostEdition,
        if host == HostKind::Linux {
            State::NotApplicable
        } else {
            State::Unknown
        },
        Code::HostEditionNotProbed,
    ));
    let arch = Architecture::from_platform_arch(std::env::consts::ARCH);
    let mut architecture = add(
        Capability::HostArchitecture,
        if matches!(arch, Architecture::Amd64 | Architecture::Arm64) {
            State::Ready
        } else {
            State::Unsupported
        },
        Code::ArchitectureDetected,
    );
    architecture.observation = Some(Observation::HostArchitecture {
        observed: arch,
        supported_runtime_architectures: supported_runtime_architectures(),
    });
    result.push(architecture);
    for (capability, applicable) in [
        (Capability::LinuxDistribution, facts.linux_distribution),
        (Capability::LinuxUidMappings, facts.linux_uid_mappings),
        (Capability::LinuxServiceManager, facts.linux_service_manager),
        (Capability::LinuxPackageManager, facts.linux_package_manager),
        (Capability::Wsl2, facts.windows_wsl2),
        (Capability::Virtualization, facts.virtualization),
    ] {
        let (state, code) = match capability {
            Capability::LinuxDistribution => (
                applicable,
                if applicable == State::Ready {
                    Code::LinuxDistributionDetected
                } else {
                    Code::HostCapabilityNotProbed
                },
            ),
            Capability::LinuxUidMappings => (
                applicable,
                match applicable {
                    State::Ready => Code::LinuxUidMappingsConfigured,
                    State::Missing => Code::LinuxUidMappingsMissing,
                    _ => Code::HostCapabilityNotProbed,
                },
            ),
            Capability::LinuxServiceManager => (
                applicable,
                if applicable == State::Ready {
                    Code::LinuxServiceManagerDetected
                } else {
                    Code::LinuxServiceManagerUnknown
                },
            ),
            Capability::LinuxPackageManager => (
                applicable,
                match applicable {
                    State::Ready => Code::LinuxPackageManagerDetected,
                    State::Missing => Code::LinuxPackageManagerMissing,
                    _ => Code::HostCapabilityNotProbed,
                },
            ),
            Capability::Wsl2 | Capability::Virtualization => {
                (applicable, Code::HostCapabilityNotProbed)
            }
            _ => (State::Unknown, Code::HostCapabilityNotProbed),
        };
        let mut item = add(capability, state, code);
        if capability == Capability::LinuxDistribution {
            item.observation = Some(Observation::LinuxDistribution {
                distribution: facts.linux_distribution_id,
                version: facts.linux_distribution_version,
            });
        }
        result.push(item);
    }
    result.push(add(
        Capability::DockerDesktopTrust,
        facts.docker_desktop_vm_trust,
        Code::DockerDesktopTrustUnverified,
    ));
}

pub(super) fn matrix(host: HostKind, facts: &HostFacts) -> HostCapabilityMatrixV1 {
    let host_arch = Architecture::from_platform_arch(std::env::consts::ARCH);
    HostCapabilityMatrixV1 {
        native_controller: native_support(
            host,
            host_arch,
            facts.linux_distribution_id,
            facts.linux_distribution_version,
        ),
        observed_host_architecture: host_arch,
        supported_runtime_architectures: supported_runtime_architectures(),
        linux_distribution: facts.linux_distribution,
        linux_distribution_id: facts.linux_distribution_id,
        linux_distribution_version: facts.linux_distribution_version,
        linux_distribution_support: facts.linux_distribution_support,
        linux_uid_mappings: facts.linux_uid_mappings,
        linux_service_manager: facts.linux_service_manager,
        linux_package_manager: facts.linux_package_manager,
        linux_rootless_mechanism: facts.linux_rootless_mechanism,
        windows_wsl2: facts.windows_wsl2,
        virtualization: facts.virtualization,
        docker_desktop_vm_trust: facts.docker_desktop_vm_trust,
    }
}

fn supported_runtime_architectures() -> Vec<Architecture> {
    ImageLock::bundled()
        .ok()
        .map(|lock| {
            [Architecture::Amd64, Architecture::Arm64]
                .into_iter()
                .filter(|arch| {
                    arch.image_name()
                        .is_some_and(|name| lock.select(RuntimeProfile::General, name).is_ok())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn distribution_support(
    host: HostKind,
    distribution: Option<Distribution>,
    version: Option<DistributionVersion>,
) -> Option<SupportClaim> {
    let supported = distribution == Some(Distribution::Ubuntu)
        && version.is_some_and(|version| version.major == 24 && version.minor == 4);
    (host == HostKind::Linux).then_some(if supported {
        SupportClaim::Experimental
    } else {
        SupportClaim::Unverified
    })
}

fn rootless_mechanism(host: HostKind, service_manager: State) -> Option<RootlessMechanism> {
    (host == HostKind::Linux).then_some(if service_manager == State::Ready {
        RootlessMechanism::SystemdUser
    } else {
        RootlessMechanism::Unknown
    })
}

fn native_support(
    host: HostKind,
    architecture: Architecture,
    distribution: Option<Distribution>,
    version: Option<DistributionVersion>,
) -> SupportClaim {
    match host {
        HostKind::Linux
            if architecture == Architecture::Amd64
                && distribution == Some(Distribution::Ubuntu)
                && version.is_some_and(|version| version.major == 24 && version.minor == 4) =>
        {
            SupportClaim::Experimental
        }
        HostKind::Linux => SupportClaim::Unverified,
        HostKind::MacOs | HostKind::Windows => SupportClaim::Planned,
        HostKind::Unsupported => SupportClaim::Unsupported,
    }
}
