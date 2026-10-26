use super::{
    PRECONDITIONS,
    mapping::{Phase, host_ready, is_sha256},
    supports,
};
use crate::app::setup_attention::catalog as setup;

fn evidence(
    state: setup::EvidenceState,
    code: setup::EvidenceCode,
    observation: Option<setup::ProbeObservationV1>,
) -> setup::ProbeEvidenceV1 {
    evidence_for(
        setup::PrerequisiteCapability::LinuxSubuid,
        state,
        code,
        observation,
    )
}

fn evidence_for(
    capability: setup::PrerequisiteCapability,
    state: setup::EvidenceState,
    code: setup::EvidenceCode,
    observation: Option<setup::ProbeObservationV1>,
) -> setup::ProbeEvidenceV1 {
    setup::ProbeEvidenceV1 {
        prerequisite: setup::PrerequisiteId {
            domain: setup::PrerequisiteDomain::Host,
            platform: setup::HostKind::Linux,
            capability,
            subject: setup::PrerequisiteSubject::None,
        },
        state,
        code,
        origin: setup::EvidenceOrigin::HostRuntime,
        scan_generation: 4,
        source_generation: None,
        observed_at_unix_ms: 120,
        observation,
    }
}

#[test]
fn accepts_only_the_exact_ordered_probe_sets() {
    let ids = |values: &[&str]| {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    };
    assert!(supports(
        &ids(&[
            "host:linux:rootless_recipe",
            "host:linux:kernel",
            "host:linux:user_namespaces",
            "host:linux:cgroups",
            "host:linux:subuid",
            "host:linux:subgid"
        ]),
        PRECONDITIONS
    ));
    assert!(!supports(
        &ids(&["host:linux:subuid", "host:linux:subgid"]),
        PRECONDITIONS
    ));
    assert!(!supports(
        &ids(&["host:linux:subuid", "host:linux:subuid"]),
        PRECONDITIONS
    ));
    assert!(!supports(&ids(&["host:linux:future_probe"]), PRECONDITIONS));
}

#[test]
fn missing_subordinate_id_is_precondition_ready_only_when_typed_missing() {
    let item = evidence(
        setup::EvidenceState::Missing,
        setup::EvidenceCode::LinuxHostCheckMissing,
        Some(setup::ProbeObservationV1::LinuxSubordinateIds { configured: false }),
    );
    assert!(host_ready("host:linux:subuid", &item, Phase::Before));
    assert!(!host_ready("host:linux:subuid", &item, Phase::After));
    let contradictory = evidence(
        setup::EvidenceState::Missing,
        setup::EvidenceCode::LinuxHostCheckMissing,
        Some(setup::ProbeObservationV1::LinuxSubordinateIds { configured: true }),
    );
    assert!(!host_ready(
        "host:linux:subuid",
        &contradictory,
        Phase::Before
    ));
}

#[test]
fn service_and_helper_checks_require_all_required_typed_facts() {
    let mut item = evidence(
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxServiceManagerDetected,
        Some(setup::ProbeObservationV1::LinuxServiceManager {
            runtime_directory_ready: true,
            private_socket_ready: true,
            bus_socket_ready: false,
        }),
    );
    assert!(!host_ready(
        "host:linux:service_manager",
        &item,
        Phase::Before
    ));
    assert!(host_ready(
        "host:linux:user_runtime_directory",
        &item,
        Phase::Before
    ));
    item.origin = setup::EvidenceOrigin::DockerDiscovery;
    assert!(!host_ready(
        "host:linux:user_runtime_directory",
        &item,
        Phase::Before
    ));
}

#[test]
fn rootless_helpers_require_each_executable_but_not_optional_slirp() {
    let item = evidence(
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxRootlessHelpers {
            newuidmap: true,
            newgidmap: true,
            rootlesskit: true,
            dockerd_rootless: true,
            setuptool: true,
            slirp4netns: Some(false),
        }),
    );
    assert!(host_ready(
        "host:linux:rootless_helpers",
        &item,
        Phase::Before
    ));
    let incomplete = evidence(
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxRootlessHelpers {
            newuidmap: true,
            newgidmap: true,
            rootlesskit: false,
            dockerd_rootless: true,
            setuptool: true,
            slirp4netns: Some(true),
        }),
    );
    assert!(!host_ready(
        "host:linux:rootless_helpers",
        &incomplete,
        Phase::Before
    ));
}

#[test]
fn mutating_rootless_preconditions_reject_unsupported_kernel_namespaces_and_cgroups() {
    let kernel = |major, minor| {
        evidence_for(
            setup::PrerequisiteCapability::LinuxKernel,
            setup::EvidenceState::Ready,
            setup::EvidenceCode::LinuxHostCheckReady,
            Some(setup::ProbeObservationV1::LinuxKernel {
                version: setup::LinuxKernelVersionV1 {
                    major,
                    minor,
                    patch: 0,
                },
            }),
        )
    };
    assert!(!host_ready(
        "host:linux:kernel",
        &kernel(4, 14),
        Phase::Before
    ));
    assert!(host_ready(
        "host:linux:kernel",
        &kernel(4, 15),
        Phase::Before
    ));
    assert!(host_ready(
        "host:linux:kernel",
        &kernel(6, 8),
        Phase::Before
    ));

    let namespaces = evidence_for(
        setup::PrerequisiteCapability::LinuxUserNamespaces,
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxUserNamespaces {
            max_user_namespaces: Some(16_000),
            unprivileged_clone_allowed: Some(true),
            apparmor_userns_restricted: Some(false),
        }),
    );
    assert!(host_ready(
        "host:linux:user_namespaces",
        &namespaces,
        Phase::Before
    ));
    let disabled_namespaces = evidence_for(
        setup::PrerequisiteCapability::LinuxUserNamespaces,
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxUserNamespaces {
            max_user_namespaces: Some(16_000),
            unprivileged_clone_allowed: Some(false),
            apparmor_userns_restricted: Some(false),
        }),
    );
    assert!(!host_ready(
        "host:linux:user_namespaces",
        &disabled_namespaces,
        Phase::Before
    ));

    let cgroups = evidence_for(
        setup::PrerequisiteCapability::LinuxCgroups,
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxCgroups {
            cgroup_v2: true,
            cpu_delegated: true,
            memory_delegated: true,
            pids_delegated: true,
        }),
    );
    assert!(host_ready("host:linux:cgroups", &cgroups, Phase::Before));
    let cgroup_v1 = evidence_for(
        setup::PrerequisiteCapability::LinuxCgroups,
        setup::EvidenceState::Ready,
        setup::EvidenceCode::LinuxHostCheckReady,
        Some(setup::ProbeObservationV1::LinuxCgroups {
            cgroup_v2: false,
            cpu_delegated: true,
            memory_delegated: true,
            pids_delegated: true,
        }),
    );
    assert!(!host_ready("host:linux:cgroups", &cgroup_v1, Phase::Before));
}

#[test]
fn fingerprints_require_lowercase_sha256_hex() {
    assert!(is_sha256(&"a5".repeat(32)));
    assert!(!is_sha256(&"A5".repeat(32)));
    assert!(!is_sha256("synthetic-endpoint"));
}
