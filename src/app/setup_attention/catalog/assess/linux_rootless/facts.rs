use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    LinuxPackageManagerV1 as PackageManager, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
    ProbeObservationV1 as Observation,
};

use super::TaskReadiness;

const MIN_CGROUP_V2_KERNEL: (u16, u16) = (4, 15);

pub(super) fn package_readiness(
    evidence: &[Evidence],
    host: HostKind,
    generation: u64,
    baseline_ready: bool,
) -> TaskReadiness {
    match helpers_state(host_fact(
        evidence,
        host,
        generation,
        Capability::LinuxRootlessHelpers,
    )) {
        State::Ready => TaskReadiness::NotRequired,
        State::Missing
            if baseline_ready
                && package_manager_ready(host_fact(
                    evidence,
                    host,
                    generation,
                    Capability::LinuxPackageManager,
                )) =>
        {
            TaskReadiness::Actionable
        }
        State::Missing | State::Unknown | State::Failed | State::Unsupported => {
            TaskReadiness::Manual
        }
        _ => TaskReadiness::Manual,
    }
}

pub(super) fn subids_readiness(
    evidence: &[Evidence],
    host: HostKind,
    generation: u64,
    baseline_ready: bool,
) -> TaskReadiness {
    let uid = subordinate_ids_state(host_fact(
        evidence,
        host,
        generation,
        Capability::LinuxSubuid,
    ));
    let gid = subordinate_ids_state(host_fact(
        evidence,
        host,
        generation,
        Capability::LinuxSubgid,
    ));
    match (uid, gid) {
        (State::Ready, State::Ready) => TaskReadiness::NotRequired,
        (State::Missing, State::Missing) if baseline_ready => TaskReadiness::Actionable,
        _ => TaskReadiness::Manual,
    }
}

pub(super) fn service_readiness(
    evidence: &[Evidence],
    host: HostKind,
    generation: u64,
    baseline_ready: bool,
    package_readiness: TaskReadiness,
    subids_readiness: TaskReadiness,
) -> TaskReadiness {
    if baseline_ready
        && package_readiness != TaskReadiness::Manual
        && subids_readiness != TaskReadiness::Manual
        && service_manager_ready(host_fact(
            evidence,
            host,
            generation,
            Capability::LinuxServiceManager,
        ))
        && cgroups_ready(host_fact(
            evidence,
            host,
            generation,
            Capability::LinuxCgroups,
        ))
    {
        TaskReadiness::Actionable
    } else {
        TaskReadiness::Manual
    }
}

pub(super) fn baseline_ready(evidence: &[Evidence], host: HostKind, generation: u64) -> bool {
    kernel_ready(host_fact(
        evidence,
        host,
        generation,
        Capability::LinuxKernel,
    )) && userns_ready(host_fact(
        evidence,
        host,
        generation,
        Capability::LinuxUserNamespaces,
    ))
}

pub(super) fn host_fact(
    evidence: &[Evidence],
    host: HostKind,
    generation: u64,
    capability: Capability,
) -> Option<&Evidence> {
    let mut matches = evidence.iter().filter(|item| {
        item.prerequisite.domain == Domain::Host
            && item.prerequisite.platform == host
            && item.prerequisite.capability == capability
            && item.prerequisite.subject == Subject::None
    });
    let item = matches.next()?;
    (matches.next().is_none()
        && item.origin == Origin::HostRuntime
        && item.scan_generation == generation
        && item.source_generation.is_none())
    .then_some(item)
}

fn kernel_ready(fact: Option<&Evidence>) -> bool {
    fact.is_some_and(|fact| {
        fact.state == State::Ready
            && fact.code == Code::LinuxHostCheckReady
            && matches!(
                &fact.observation,
                Some(Observation::LinuxKernel { version })
                    if version.major > MIN_CGROUP_V2_KERNEL.0
                        || (version.major == MIN_CGROUP_V2_KERNEL.0
                            && version.minor >= MIN_CGROUP_V2_KERNEL.1)
            )
    })
}

fn userns_ready(fact: Option<&Evidence>) -> bool {
    fact.is_some_and(|fact| {
        fact.state == State::Ready
            && fact.code == Code::LinuxHostCheckReady
            && matches!(
                &fact.observation,
                Some(Observation::LinuxUserNamespaces {
                    max_user_namespaces: Some(limit),
                    unprivileged_clone_allowed,
                    apparmor_userns_restricted,
                }) if *limit > 0
                    && *unprivileged_clone_allowed != Some(false)
                    && *apparmor_userns_restricted != Some(true)
            )
    })
}

fn helpers_state(fact: Option<&Evidence>) -> State {
    let Some(fact) = fact else {
        return State::Unknown;
    };
    let Some(Observation::LinuxRootlessHelpers {
        newuidmap,
        newgidmap,
        rootlesskit,
        dockerd_rootless,
        setuptool,
        ..
    }) = &fact.observation
    else {
        return State::Unknown;
    };
    let complete = *newuidmap && *newgidmap && *rootlesskit && *dockerd_rootless && *setuptool;
    match (fact.state, fact.code) {
        (State::Ready, Code::LinuxHostCheckReady) if complete => State::Ready,
        (State::Missing, Code::LinuxHostCheckMissing) if !complete => State::Missing,
        (State::Failed, Code::LinuxProbeMalformed) => State::Failed,
        (State::Unsupported, Code::LinuxProbeUnsupported) => State::Unsupported,
        (State::Unknown, Code::LinuxProbeInaccessible | Code::HostCapabilityNotProbed) => {
            State::Unknown
        }
        _ => State::Unknown,
    }
}

fn subordinate_ids_state(fact: Option<&Evidence>) -> State {
    let Some(fact) = fact else {
        return State::Unknown;
    };
    let Some(Observation::LinuxSubordinateIds { configured }) = &fact.observation else {
        return State::Unknown;
    };
    match (fact.state, fact.code, *configured) {
        (State::Ready, Code::LinuxHostCheckReady, true) => State::Ready,
        (State::Missing, Code::LinuxHostCheckMissing, false) => State::Missing,
        (State::Unknown, Code::LinuxProbeInaccessible | Code::HostCapabilityNotProbed, false) => {
            State::Unknown
        }
        (State::Failed, Code::LinuxProbeMalformed, false) => State::Failed,
        _ => State::Unknown,
    }
}

fn package_manager_ready(fact: Option<&Evidence>) -> bool {
    fact.is_some_and(|fact| {
        fact.state == State::Ready
            && fact.code == Code::LinuxPackageManagerDetected
            && matches!(
                &fact.observation,
                Some(Observation::LinuxPackageManager {
                    expected: Some(expected),
                    found: Some(found),
                }) if expected == found
                    && matches!(*expected, PackageManager::AptGet | PackageManager::Dnf)
            )
    })
}

fn service_manager_ready(fact: Option<&Evidence>) -> bool {
    fact.is_some_and(|fact| {
        fact.state == State::Ready
            && fact.code == Code::LinuxServiceManagerDetected
            && matches!(
                &fact.observation,
                Some(Observation::LinuxServiceManager {
                    runtime_directory_ready: true,
                    private_socket_ready: true,
                    bus_socket_ready: true,
                })
            )
    })
}

fn cgroups_ready(fact: Option<&Evidence>) -> bool {
    fact.is_some_and(|fact| {
        fact.state == State::Ready
            && fact.code == Code::LinuxHostCheckReady
            && matches!(
                &fact.observation,
                Some(Observation::LinuxCgroups {
                    cgroup_v2: true,
                    cpu_delegated: true,
                    memory_delegated: true,
                    pids_delegated: true,
                })
            )
    })
}
