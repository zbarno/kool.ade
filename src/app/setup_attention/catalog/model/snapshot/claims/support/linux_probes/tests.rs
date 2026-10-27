use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxDistributionVersionV1 as Version, LinuxKernelVersionV1,
    LinuxPackageManagerV1 as PackageManager, LinuxRootlessMechanismV1 as Mechanism,
    LinuxRootlessRecipeV1 as Recipe, MachineSnapshotV1, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteId, PrerequisiteSubject as Subject,
    ProbeEvidenceV1 as Evidence, ProbeObservationV1 as Observation, SupportClaim,
};

fn valid_snapshot() -> MachineSnapshotV1 {
    let observations = [
        (
            Capability::LinuxRootlessRecipe,
            Code::LinuxRootlessRecipeCandidate,
            Some(Observation::LinuxRootlessRecipe {
                candidate: Some(Recipe::Ubuntu2404Amd64),
                distribution_state: State::Ready,
            }),
        ),
        (
            Capability::LinuxKernel,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxKernel {
                version: LinuxKernelVersionV1 {
                    major: 6,
                    minor: 8,
                    patch: 0,
                },
            }),
        ),
        (
            Capability::LinuxUserNamespaces,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxUserNamespaces {
                max_user_namespaces: Some(28_633),
                unprivileged_clone_allowed: Some(true),
                apparmor_userns_restricted: Some(false),
            }),
        ),
        (
            Capability::LinuxSubuid,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
        (
            Capability::LinuxSubgid,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
        (
            Capability::LinuxRootlessHelpers,
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
        (
            Capability::LinuxCgroups,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxCgroups {
                cgroup_v2: true,
                cpu_delegated: true,
                memory_delegated: true,
                pids_delegated: true,
            }),
        ),
        (
            Capability::LinuxUidMappings,
            Code::LinuxUidMappingsConfigured,
            None,
        ),
        (
            Capability::LinuxServiceManager,
            Code::LinuxServiceManagerDetected,
            Some(Observation::LinuxServiceManager {
                runtime_directory_ready: true,
                private_socket_ready: true,
                bus_socket_ready: true,
            }),
        ),
        (
            Capability::LinuxPackageManager,
            Code::LinuxPackageManagerDetected,
            Some(Observation::LinuxPackageManager {
                expected: Some(PackageManager::AptGet),
                found: Some(PackageManager::AptGet),
            }),
        ),
    ];
    let mut probe_evidence: Vec<Evidence> = observations
        .into_iter()
        .map(|(capability, code, observation)| Evidence {
            prerequisite: PrerequisiteId {
                domain: Domain::Host,
                platform: HostKind::Linux,
                capability,
                subject: Subject::None,
            },
            state: State::Ready,
            code,
            origin: Origin::HostRuntime,
            scan_generation: 2,
            source_generation: None,
            observed_at_unix_ms: 100,
            observation,
        })
        .collect();
    probe_evidence.extend(
        [
            (
                Capability::HostFamily,
                State::Ready,
                Code::RuntimeTargetDetected,
                None,
            ),
            (
                Capability::HostArchitecture,
                State::Ready,
                Code::ArchitectureDetected,
                Some(Observation::HostArchitecture {
                    observed: Architecture::Amd64,
                    supported_runtime_architectures: vec![Architecture::Amd64],
                }),
            ),
            (
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
            (
                Capability::Wsl2,
                State::NotApplicable,
                Code::HostCapabilityNotProbed,
                None,
            ),
            (
                Capability::Virtualization,
                State::NotApplicable,
                Code::HostCapabilityNotProbed,
                None,
            ),
            (
                Capability::DockerDesktopTrust,
                State::NotApplicable,
                Code::DockerDesktopTrustUnverified,
                None,
            ),
        ]
        .into_iter()
        .map(|(capability, state, code, observation)| Evidence {
            prerequisite: PrerequisiteId {
                domain: Domain::Host,
                platform: HostKind::Linux,
                capability,
                subject: Subject::None,
            },
            state,
            code,
            origin: Origin::HostRuntime,
            scan_generation: 2,
            source_generation: None,
            observed_at_unix_ms: 100,
            observation,
        }),
    );
    MachineSnapshotV1 {
        schema_version: MachineSnapshotV1::SCHEMA_VERSION,
        scan_generation: 2,
        scan_started_at_unix_ms: 100,
        host: HostKind::Linux,
        selected_provider: None,
        checked_at_unix_ms: 100,
        capability_matrix: HostCapabilityMatrixV1 {
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
        },
        probe_evidence,
        prerequisite_graph: Vec::new(),
        findings: Vec::new(),
        checked_domains: Vec::new(),
    }
}

#[test]
fn detailed_linux_probe_evidence_validates_against_exact_host_identity() {
    assert!(super::validate(&valid_snapshot()).is_ok());
}

#[test]
fn legacy_schema_v1_unknown_linux_facts_keep_their_original_evidence_codes() {
    let mut snapshot = valid_snapshot();
    snapshot
        .probe_evidence
        .retain(|item| !super::NEW_FACTS.contains(&item.prerequisite.capability));

    snapshot.capability_matrix.linux_uid_mappings = State::Unknown;
    snapshot.capability_matrix.linux_service_manager = State::Unknown;
    snapshot.capability_matrix.linux_rootless_mechanism = Some(Mechanism::Unknown);

    let uid_mappings = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == Capability::LinuxUidMappings)
        .expect("UID mappings evidence");
    uid_mappings.state = State::Unknown;
    uid_mappings.code = Code::HostCapabilityNotProbed;
    uid_mappings.observation = None;

    let service_manager = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == Capability::LinuxServiceManager)
        .expect("service manager evidence");
    service_manager.state = State::Unknown;
    service_manager.code = Code::LinuxServiceManagerUnknown;
    service_manager.observation = None;

    assert!(super::super::validate_capability_matrix(&snapshot).is_ok());
}

#[test]
fn detailed_linux_probe_snapshots_reject_legacy_unknown_evidence_codes() {
    for (capability, code) in [
        (Capability::LinuxUidMappings, Code::HostCapabilityNotProbed),
        (
            Capability::LinuxServiceManager,
            Code::LinuxServiceManagerUnknown,
        ),
    ] {
        let mut snapshot = valid_snapshot();
        match capability {
            Capability::LinuxUidMappings => {
                snapshot.capability_matrix.linux_uid_mappings = State::Unknown;
            }
            Capability::LinuxServiceManager => {
                snapshot.capability_matrix.linux_service_manager = State::Unknown;
                snapshot.capability_matrix.linux_rootless_mechanism = Some(Mechanism::Unknown);
            }
            _ => unreachable!("test cases only include legacy Linux facts"),
        }
        let evidence = snapshot
            .probe_evidence
            .iter_mut()
            .find(|item| item.prerequisite.capability == capability)
            .expect("legacy capability evidence");
        evidence.state = State::Unknown;
        evidence.code = code;
        evidence.observation = None;

        assert!(super::super::validate_capability_matrix(&snapshot).is_err());
    }
}

#[test]
fn detailed_linux_probe_validator_rejects_recipe_and_package_manager_mismatch() {
    let mut snapshot = valid_snapshot();
    let recipe = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == Capability::LinuxRootlessRecipe)
        .expect("recipe evidence");
    recipe.observation = Some(Observation::LinuxRootlessRecipe {
        candidate: None,
        distribution_state: State::Ready,
    });
    assert!(super::validate(&snapshot).is_err());

    let mut snapshot = valid_snapshot();
    let package = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == Capability::LinuxPackageManager)
        .expect("package manager evidence");
    package.observation = Some(Observation::LinuxPackageManager {
        expected: Some(PackageManager::Dnf),
        found: Some(PackageManager::Dnf),
    });
    assert!(super::validate(&snapshot).is_err());

    let mut snapshot = valid_snapshot();
    let kernel = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == Capability::LinuxKernel)
        .expect("kernel evidence");
    kernel.code = Code::LinuxPackageManagerDetected;
    assert!(super::validate(&snapshot).is_err());
}
