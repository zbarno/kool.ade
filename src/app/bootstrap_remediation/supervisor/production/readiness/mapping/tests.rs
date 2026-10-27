use super::super::{
    ReadinessSource, SERVICE_VERIFY,
    system::{EndpointObservation, ProbeOutcome},
};
use super::{Phase, ReceiptEvidence, collect};
use crate::app::setup_attention::{SetupScanResult, catalog as setup};
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

const GENERATION: u64 = 7;
const ENDPOINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DAEMON: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const READINESS: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct Source {
    scan: SetupScanResult,
}

impl ReadinessSource for Source {
    fn scan(&self) -> anyhow::Result<SetupScanResult> {
        Ok(self.scan.clone())
    }

    fn service_absent(&self) -> ProbeOutcome {
        ProbeOutcome::Ready
    }

    fn rootlesskit_smoke(&self) -> ProbeOutcome {
        ProbeOutcome::Ready
    }

    fn endpoint(&self) -> EndpointObservation {
        EndpointObservation {
            trusted: true,
            fingerprint: Some(ENDPOINT.into()),
        }
    }
}

fn evidence(
    domain: setup::PrerequisiteDomain,
    capability: setup::PrerequisiteCapability,
    subject: setup::PrerequisiteSubject,
    code: setup::EvidenceCode,
    origin: setup::EvidenceOrigin,
    observation: Option<setup::ProbeObservationV1>,
) -> setup::ProbeEvidenceV1 {
    setup::ProbeEvidenceV1 {
        prerequisite: setup::PrerequisiteId {
            domain,
            platform: setup::HostKind::Linux,
            capability,
            subject,
        },
        state: setup::EvidenceState::Ready,
        code,
        origin,
        scan_generation: GENERATION,
        source_generation: None,
        observed_at_unix_ms: 50,
        observation,
    }
}

fn source(with_smoke: bool) -> Source {
    use setup::{
        EvidenceCode as Code, EvidenceOrigin as Origin, ImageArchitectureV1 as Architecture,
        ImageProfileV1 as Profile, LinuxRootlessRecipeV1 as Recipe,
        PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
        PrerequisiteSubject as Subject, ProbeObservationV1 as Observation,
    };

    let mut items = vec![
        evidence(
            Domain::Host,
            Capability::LinuxRootlessRecipe,
            Subject::None,
            Code::LinuxRootlessRecipeCandidate,
            Origin::HostRuntime,
            Some(Observation::LinuxRootlessRecipe {
                candidate: Some(Recipe::Ubuntu2404Amd64),
                distribution_state: setup::EvidenceState::Ready,
            }),
        ),
        evidence(
            Domain::Host,
            Capability::LinuxRootlessHelpers,
            Subject::None,
            Code::LinuxHostCheckReady,
            Origin::HostRuntime,
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
            Domain::Host,
            Capability::LinuxServiceManager,
            Subject::None,
            Code::LinuxServiceManagerDetected,
            Origin::HostRuntime,
            Some(Observation::LinuxServiceManager {
                runtime_directory_ready: true,
                private_socket_ready: true,
                bus_socket_ready: true,
            }),
        ),
    ];
    for capability in [
        Capability::DockerEngine,
        Capability::DockerEndpoint,
        Capability::DockerRootless,
    ] {
        items.push(evidence(
            Domain::Docker,
            capability,
            Subject::None,
            Code::DockerReady,
            Origin::DockerDiscovery,
            None,
        ));
    }
    let mut checked_domains = vec!["docker".into()];
    if with_smoke {
        let image = ImageLock::bundled()
            .unwrap()
            .select(RuntimeProfile::General, "amd64")
            .unwrap();
        items.push(evidence(
            Domain::Docker,
            Capability::DockerRootlessSmoke,
            Subject::Image {
                profile: Profile::General,
                architecture: Architecture::Amd64,
            },
            Code::ApprovedImageSmokeSucceeded,
            Origin::ApprovedImageSmokeTest,
            Some(Observation::DockerRootlessSmoke {
                endpoint_fingerprint: ENDPOINT.into(),
                daemon_fingerprint: DAEMON.into(),
                daemon_version: "27.1.2".into(),
                image_reference: image.reference,
                image_digest: image.digest,
                image_platform: image.platform,
                completed_at_unix_ms: 40,
            }),
        ));
        checked_domains.push("docker:rootless-smoke".into());
    }

    Source {
        scan: SetupScanResult {
            generation: GENERATION,
            issue: None,
            snapshot: Some(setup::MachineSnapshotV1 {
                schema_version: setup::SCHEMA_VERSION,
                scan_generation: GENERATION,
                scan_started_at_unix_ms: 1,
                host: setup::HostKind::Linux,
                setup_provider: None,
                selected_provider: None,
                checked_at_unix_ms: 100,
                capability_matrix: setup::HostCapabilityMatrixV1 {
                    native_controller: setup::SupportClaim::Experimental,
                    observed_host_architecture: Architecture::Amd64,
                    supported_runtime_architectures: vec![Architecture::Amd64],
                    linux_distribution: setup::EvidenceState::Ready,
                    linux_distribution_id: Some(setup::LinuxDistributionV1::Ubuntu),
                    linux_distribution_version: Some(setup::LinuxDistributionVersionV1 {
                        major: 24,
                        minor: 4,
                    }),
                    linux_distribution_support: Some(setup::SupportClaim::Experimental),
                    linux_uid_mappings: setup::EvidenceState::Ready,
                    linux_service_manager: setup::EvidenceState::Ready,
                    linux_package_manager: setup::EvidenceState::Ready,
                    linux_rootless_mechanism: Some(setup::LinuxRootlessMechanismV1::SystemdUser),
                    windows_wsl2: setup::EvidenceState::NotApplicable,
                    virtualization: setup::EvidenceState::NotApplicable,
                    docker_desktop_vm_trust: setup::EvidenceState::NotApplicable,
                },
                probe_evidence: items,
                prerequisite_graph: Vec::new(),
                findings: Vec::new(),
                checked_domains,
            }),
            endpoint_fingerprint: Some(ENDPOINT.into()),
            daemon_identity_fingerprint: Some(DAEMON.into()),
            docker_readiness_fingerprint: Some(READINESS.into()),
            setup_provider: None,
            selected_provider: None,
            cancelled: false,
        },
    }
}

fn verify_service(source: &impl ReadinessSource) -> ReceiptEvidence {
    collect(
        source,
        &SERVICE_VERIFY
            .iter()
            .map(|id| (*id).to_owned())
            .collect::<Vec<_>>(),
        Phase::After,
    )
    .unwrap()
}

#[test]
fn service_operation_can_await_smoke_but_cannot_verify_from_engine_readiness_alone() {
    let engine_only = verify_service(&source(false));
    assert!(!engine_only.ready);
    assert!(engine_only.service_ready_awaiting_smoke);

    let smoked = verify_service(&source(true));
    assert!(smoked.ready);
    assert!(!smoked.service_ready_awaiting_smoke);
}
