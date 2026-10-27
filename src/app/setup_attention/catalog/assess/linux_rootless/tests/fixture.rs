use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxDistributionVersionV1 as Version, LinuxKernelVersionV1,
    LinuxPackageManagerV1 as PackageManager, LinuxRootlessMechanismV1 as Mechanism,
    LinuxRootlessRecipeV1 as Recipe, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteId, PrerequisiteSubject as Subject,
    ProbeEvidenceV1 as Evidence, ProbeObservationV1 as Observation, SupportClaim,
};

pub(super) const GENERATION: u64 = 4;

pub(super) fn candidate_matrix() -> HostCapabilityMatrixV1 {
    HostCapabilityMatrixV1 {
        native_controller: SupportClaim::Experimental,
        observed_host_architecture: Architecture::Amd64,
        supported_runtime_architectures: vec![Architecture::Amd64],
        linux_distribution: State::Ready,
        linux_distribution_id: Some(Distribution::Ubuntu),
        linux_distribution_version: Some(Version {
            major: 24,
            minor: 4,
        }),
        linux_distribution_support: Some(SupportClaim::Experimental),
        linux_uid_mappings: State::Ready,
        linux_service_manager: State::Ready,
        linux_package_manager: State::Ready,
        linux_rootless_mechanism: Some(Mechanism::SystemdUser),
        windows_wsl2: State::NotApplicable,
        virtualization: State::NotApplicable,
        docker_desktop_vm_trust: State::NotApplicable,
    }
}

fn evidence(
    capability: Capability,
    state: State,
    code: Code,
    observation: Option<Observation>,
) -> Evidence {
    Evidence {
        prerequisite: PrerequisiteId {
            domain: Domain::Host,
            platform: HostKind::Linux,
            capability,
            subject: Subject::None,
        },
        state,
        code,
        origin: Origin::HostRuntime,
        scan_generation: GENERATION,
        source_generation: None,
        observed_at_unix_ms: 100,
        observation,
    }
}

pub(super) fn candidate_evidence() -> Vec<Evidence> {
    vec![
        evidence(
            Capability::LinuxDistribution,
            State::Ready,
            Code::LinuxDistributionDetected,
            Some(Observation::LinuxDistribution {
                distribution: Some(Distribution::Ubuntu),
                version: Some(Version {
                    major: 24,
                    minor: 4,
                }),
            }),
        ),
        evidence(
            Capability::LinuxRootlessRecipe,
            State::Ready,
            Code::LinuxRootlessRecipeCandidate,
            Some(Observation::LinuxRootlessRecipe {
                candidate: Some(Recipe::Ubuntu2404Amd64),
                distribution_state: State::Ready,
            }),
        ),
        evidence(
            Capability::LinuxKernel,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxKernel {
                version: LinuxKernelVersionV1 {
                    major: 6,
                    minor: 8,
                    patch: 0,
                },
            }),
        ),
        evidence(
            Capability::LinuxUserNamespaces,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxUserNamespaces {
                max_user_namespaces: Some(28_633),
                unprivileged_clone_allowed: Some(true),
                apparmor_userns_restricted: Some(false),
            }),
        ),
        evidence(
            Capability::LinuxSubuid,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
        evidence(
            Capability::LinuxSubgid,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
        evidence(
            Capability::LinuxRootlessHelpers,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxRootlessHelpers {
                newuidmap: true,
                newgidmap: true,
                rootlesskit: true,
                dockerd_rootless: true,
                setuptool: true,
                slirp4netns: Some(true),
            }),
        ),
        evidence(
            Capability::LinuxCgroups,
            State::Ready,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxCgroups {
                cgroup_v2: true,
                cpu_delegated: true,
                memory_delegated: true,
                pids_delegated: true,
            }),
        ),
        evidence(
            Capability::LinuxServiceManager,
            State::Ready,
            Code::LinuxServiceManagerDetected,
            Some(Observation::LinuxServiceManager {
                runtime_directory_ready: true,
                private_socket_ready: true,
                bus_socket_ready: true,
            }),
        ),
        evidence(
            Capability::LinuxPackageManager,
            State::Ready,
            Code::LinuxPackageManagerDetected,
            Some(Observation::LinuxPackageManager {
                expected: Some(PackageManager::AptGet),
                found: Some(PackageManager::AptGet),
            }),
        ),
    ]
}

pub(super) fn set_fact(
    evidence: &mut [Evidence],
    capability: Capability,
    state: State,
    code: Code,
    observation: Option<Observation>,
) {
    let item = evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == capability)
        .expect("host evidence fixture");
    item.state = state;
    item.code = code;
    item.observation = observation;
}
