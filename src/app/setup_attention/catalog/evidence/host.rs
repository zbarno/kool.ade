use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxDistributionVersionV1 as DistributionVersion,
    LinuxRootlessMechanismV1 as RootlessMechanism, LinuxRootlessRecipeV1 as RootlessRecipe,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence, ProbeObservationV1 as Observation,
    SupportClaim,
};
use crate::execution::docker::environment::DockerEnvironmentStatus;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

// This module retains Linux probe types for shared evidence, while only Linux
// builds execute the host inspection implementation.
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
    linux_probe: probe::LinuxProbeFacts,
    windows: probe::WindowsFacts,
    linux_rootless_mechanism: Option<RootlessMechanism>,
    windows_wsl2: State,
    virtualization: State,
    docker_desktop_vm_trust: State,
    macos_release: Option<String>,
}

pub(super) fn probe(host: HostKind, docker: &DockerEnvironmentStatus) -> HostFacts {
    let linux = host == HostKind::Linux;
    let desktop = matches!(host, HostKind::MacOs | HostKind::Windows) || docker.desktop_vm;
    let linux_probe = probe::collect(
        host,
        Architecture::from_platform_arch(std::env::consts::ARCH),
    );
    #[cfg(target_os = "macos")]
    let macos_release = (host == HostKind::MacOs)
        .then(macos::product_version)
        .flatten();
    #[cfg(not(target_os = "macos"))]
    let macos_release = None;
    let distribution = linux_probe.distribution;
    let windows = if host == HostKind::Windows {
        probe::collect_windows()
    } else {
        probe::WindowsFacts::not_applicable()
    };
    let linux_distribution_id = distribution.map(|(id, _)| id);
    let linux_distribution_version = distribution.and_then(|(_, version)| version);
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
            linux_probe.uid_mappings.state
        } else {
            State::NotApplicable
        },
        linux_service_manager: linux_probe.service_manager.state,
        linux_package_manager: if linux {
            linux_probe.package_manager.state
        } else {
            State::NotApplicable
        },
        linux_rootless_mechanism: rootless_mechanism(host, linux_probe.service_manager.state),
        linux_probe,
        windows_wsl2: if host == HostKind::Windows {
            windows.wsl2_state()
        } else {
            State::NotApplicable
        },
        virtualization: if host == HostKind::Windows {
            windows.virtualization.state
        } else if host == HostKind::MacOs {
            State::Unknown
        } else {
            State::NotApplicable
        },
        windows,
        docker_desktop_vm_trust: if !desktop {
            State::NotApplicable
        } else if matches!(host, HostKind::MacOs | HostKind::Windows)
            && docker.desktop_vm
            && docker.desktop_transport_verified
        {
            State::Ready
        } else {
            State::Unknown
        },
        macos_release,
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
    let host_edition = if host == HostKind::Windows {
        (facts.windows.host.state, facts.windows.host.code)
    } else if host == HostKind::Linux {
        (State::NotApplicable, Code::HostEditionNotProbed)
    } else if host == HostKind::MacOs && facts.macos_release.is_some() {
        (State::Ready, Code::MacOsReleaseDetected)
    } else {
        (State::Unknown, Code::HostEditionNotProbed)
    };
    let mut edition = add(Capability::HostEdition, host_edition.0, host_edition.1);
    if let Some(version) = &facts.macos_release {
        edition.observation = Some(Observation::MacOsVersion {
            version: version.clone(),
        });
    }
    if host == HostKind::Windows {
        edition.observation = facts.windows.host.observation.clone();
    }
    result.push(edition);
    let arch = host_architecture(host);
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
        (Capability::Wsl2, facts.windows_wsl2),
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
            Capability::Wsl2 => (applicable, Code::HostCapabilityNotProbed),
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
    probe::push_evidence(result, host, generation, now, &facts.linux_probe);
    for (capability, probe_fact) in [
        (Capability::WindowsWslFeature, &facts.windows.wsl_feature),
        (
            Capability::WindowsVmPlatformFeature,
            &facts.windows.vm_platform_feature,
        ),
        (Capability::WindowsWslKernel, &facts.windows.wsl_kernel),
        (Capability::WindowsReboot, &facts.windows.reboot),
        (Capability::WindowsWslPolicy, &facts.windows.policy),
    ] {
        let mut item = add(capability, probe_fact.state, probe_fact.code);
        item.observation = probe_fact.observation.clone();
        result.push(item);
    }
    let (virtualization_state, virtualization_code, virtualization_observation) =
        if host == HostKind::Windows {
            (
                facts.windows.virtualization.state,
                facts.windows.virtualization.code,
                facts.windows.virtualization.observation.clone(),
            )
        } else {
            (facts.virtualization, Code::HostCapabilityNotProbed, None)
        };
    let mut virtualization = add(
        Capability::Virtualization,
        virtualization_state,
        virtualization_code,
    );
    virtualization.observation = virtualization_observation;
    result.push(virtualization);
    result.push(add(
        Capability::DockerDesktopTrust,
        facts.docker_desktop_vm_trust,
        if facts.docker_desktop_vm_trust == State::Ready {
            Code::DockerDesktopTrustVerified
        } else {
            Code::DockerDesktopTrustUnverified
        },
    ));
}

pub(super) fn matrix(host: HostKind, facts: &HostFacts) -> HostCapabilityMatrixV1 {
    let host_arch = host_architecture(host);
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

fn host_architecture(host: HostKind) -> Architecture {
    #[cfg(target_os = "macos")]
    if host == HostKind::MacOs {
        return macos::physical_architecture();
    }
    let _ = host;
    Architecture::from_platform_arch(std::env::consts::ARCH)
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
    let supported = RootlessRecipe::for_host(distribution, version, Architecture::Amd64).is_some();
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
            if RootlessRecipe::for_host(distribution, version, architecture).is_some() =>
        {
            SupportClaim::Experimental
        }
        HostKind::Linux => SupportClaim::Unverified,
        HostKind::MacOs | HostKind::Windows => SupportClaim::Planned,
        HostKind::Unsupported => SupportClaim::Unsupported,
    }
}
