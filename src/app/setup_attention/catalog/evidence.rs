use crate::{
    execution::docker::environment::DockerEnvironmentStatus, harness::discovery::ProbeResult,
};

use super::{GitProbeV1, HostCapabilityMatrixV1, HostKind, PrerequisiteNodeV1, ProbeEvidenceV1};

mod docker;
mod git;
mod graph;
mod host;
mod images;
mod providers;
#[cfg(test)]
mod tests;

pub(super) use git::probe as probe_git;

pub(super) fn id(
    domain: super::PrerequisiteDomain,
    host: HostKind,
    capability: super::PrerequisiteCapability,
    subject: super::PrerequisiteSubject,
) -> super::PrerequisiteId {
    super::PrerequisiteId {
        domain,
        platform: host,
        capability,
        subject,
    }
}

pub(super) fn evidence(
    prerequisite: super::PrerequisiteId,
    state: super::EvidenceState,
    code: super::EvidenceCode,
    origin: super::EvidenceOrigin,
    generation: u64,
    source_generation: Option<u64>,
    observed_at_unix_ms: u64,
) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite,
        state,
        code,
        origin,
        scan_generation: generation,
        source_generation,
        observed_at_unix_ms,
        observation: None,
    }
}

pub(super) fn collect(
    probes: &[ProbeResult],
    docker_status: &DockerEnvironmentStatus,
    host_kind: HostKind,
    generation: u64,
    checked_at_unix_ms: u64,
    git_probe: GitProbeV1,
) -> (
    HostCapabilityMatrixV1,
    Vec<ProbeEvidenceV1>,
    Vec<PrerequisiteNodeV1>,
) {
    let mut evidence = Vec::new();
    let host_facts = host::probe(host_kind, docker_status);
    host::push(
        &mut evidence,
        host_kind,
        generation,
        checked_at_unix_ms,
        &host_facts,
    );
    git::push(
        &mut evidence,
        host_kind,
        generation,
        checked_at_unix_ms,
        git_probe,
    );
    providers::push(
        &mut evidence,
        probes,
        host_kind,
        generation,
        checked_at_unix_ms,
    );
    docker::push(
        &mut evidence,
        docker_status,
        host_kind,
        generation,
        checked_at_unix_ms,
    );
    images::push(
        &mut evidence,
        docker_status,
        host_kind,
        generation,
        checked_at_unix_ms,
    );
    evidence.sort_by(|left, right| left.prerequisite.cmp(&right.prerequisite));
    let graph = graph::build(&evidence, host_kind);
    let matrix = host::matrix(host_kind, &host_facts);
    (matrix, evidence, graph)
}
