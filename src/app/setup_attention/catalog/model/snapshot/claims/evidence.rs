use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
};

pub(super) fn ready_claim(
    domain: Domain,
    capability: Capability,
    origin: Origin,
    code: Code,
) -> bool {
    matches!(
        (domain, capability, origin, code),
        (
            Domain::Provider,
            Capability::ProviderExecutable,
            Origin::ProviderCli,
            Code::ProviderInstalled,
        ) | (
            Domain::Provider,
            Capability::ProviderVersion,
            Origin::ProviderCli,
            Code::ProviderVersionDetected,
        ) | (
            Domain::Provider,
            Capability::ProviderAuthentication,
            Origin::ProviderCli,
            Code::AuthenticationVerified,
        ) | (
            Domain::Provider,
            Capability::ProviderModels,
            Origin::ProviderCli,
            Code::ModelCatalogVerified,
        ) | (
            Domain::Provider,
            Capability::ProviderContextOnly,
            Origin::ProviderCli,
            Code::ContextOnlyVerified,
        ) | (
            Domain::Git,
            Capability::GitExecutable,
            Origin::GitCli,
            Code::GitAvailable
        ) | (
            Domain::Git,
            Capability::GitIdentity,
            Origin::GitCli,
            Code::GitIdentityConfigured
        ) | (
            Domain::Docker,
            Capability::DockerEngine,
            Origin::DockerDiscovery,
            Code::DockerReady
        ) | (
            Domain::Docker,
            Capability::DockerEndpoint,
            Origin::DockerDiscovery,
            Code::DockerReady,
        ) | (
            Domain::Docker,
            Capability::DockerDesktopVm,
            Origin::DockerDiscovery,
            Code::DockerDesktopVmDetected,
        ) | (
            Domain::Docker,
            Capability::DockerRootless,
            Origin::DockerDiscovery,
            Code::DockerReady,
        ) | (
            Domain::Docker,
            Capability::DockerCgroups,
            Origin::DockerDiscovery,
            Code::DockerCgroupsSupported,
        ) | (
            Domain::Docker,
            Capability::DockerCapacity,
            Origin::DockerDiscovery,
            Code::DockerReady,
        ) | (
            Domain::Docker,
            Capability::DockerSecurity,
            Origin::DockerDiscovery,
            Code::DockerReady,
        ) | (
            Domain::Image,
            Capability::ApprovedImage,
            Origin::ImageInventory,
            Code::ImagePresent
        )
    )
}

pub(super) fn provider_evidence(
    capability: Capability,
    state: State,
    origin: Origin,
    code: Code,
) -> bool {
    if origin != Origin::ProviderCli {
        return false;
    }
    if state == State::Ready {
        return ready_claim(Domain::Provider, capability, origin, code);
    }
    matches!(
        (capability, state, code),
        (
            Capability::ProviderExecutable,
            State::Missing,
            Code::ProviderMissing
        ) | (
            Capability::ProviderExecutable,
            State::Unknown,
            Code::ProviderProbeFailed
        ) | (
            Capability::ProviderExecutable,
            State::Failed,
            Code::ProviderProbeFailed
        ) | (
            Capability::ProviderExecutable,
            State::Stale,
            Code::StaleProbeGeneration
        ) | (
            Capability::ProviderVersion,
            State::Unknown,
            Code::ProviderVersionUnknown
        ) | (
            Capability::ProviderVersion,
            State::Failed,
            Code::ProviderProbeFailed
        ) | (
            Capability::ProviderVersion,
            State::Stale,
            Code::StaleProbeGeneration
        ) | (
            Capability::ProviderAuthentication,
            State::Missing,
            Code::AuthenticationRequired
        ) | (
            Capability::ProviderAuthentication,
            State::Unknown,
            Code::AuthenticationUnknown
        ) | (
            Capability::ProviderAuthentication,
            State::Stale,
            Code::StaleProbeGeneration
        ) | (
            Capability::ProviderModels,
            State::Unknown,
            Code::ModelCatalogUnavailable
        ) | (
            Capability::ProviderModels,
            State::Stale,
            Code::ModelCatalogStale
        ) | (
            Capability::ProviderModels,
            State::Stale,
            Code::StaleProbeGeneration
        ) | (
            Capability::ProviderContextOnly,
            State::Missing,
            Code::ContextOnlyUnavailable
        ) | (
            Capability::ProviderContextOnly,
            State::Unknown,
            Code::ContextOnlyUnknown
        ) | (
            Capability::ProviderContextOnly,
            State::Stale,
            Code::StaleProbeGeneration
        )
    )
}
