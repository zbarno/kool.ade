use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
};

use super::super::MachineSnapshotV1;

#[cfg(test)]
#[path = "linux_probes/tests.rs"]
mod tests;
mod typed;

const NEW_FACTS: [Capability; 7] = [
    Capability::LinuxRootlessRecipe,
    Capability::LinuxKernel,
    Capability::LinuxUserNamespaces,
    Capability::LinuxSubuid,
    Capability::LinuxSubgid,
    Capability::LinuxRootlessHelpers,
    Capability::LinuxCgroups,
];

pub(super) fn has_new_facts(snapshot: &MachineSnapshotV1) -> bool {
    snapshot.probe_evidence.iter().any(|item| {
        item.prerequisite.domain == Domain::Host
            && NEW_FACTS.contains(&item.prerequisite.capability)
    })
}

pub(super) fn validate(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    let present = snapshot
        .probe_evidence
        .iter()
        .filter(|item| {
            item.prerequisite.domain == Domain::Host
                && NEW_FACTS.contains(&item.prerequisite.capability)
        })
        .count();
    if present == 0 {
        return Ok(()); // Older schema-v1 snapshots predate the detailed Linux probe facts.
    }
    anyhow::ensure!(
        present == NEW_FACTS.len(),
        "Linux host probe evidence is incomplete"
    );

    let facts = [
        Capability::LinuxRootlessRecipe,
        Capability::LinuxKernel,
        Capability::LinuxUserNamespaces,
        Capability::LinuxSubuid,
        Capability::LinuxSubgid,
        Capability::LinuxRootlessHelpers,
        Capability::LinuxCgroups,
        Capability::LinuxUidMappings,
        Capability::LinuxServiceManager,
        Capability::LinuxPackageManager,
    ]
    .into_iter()
    .map(|capability| {
        let evidence = host_fact(snapshot, capability)?;
        anyhow::ensure!(
            evidence.scan_generation == snapshot.scan_generation
                && evidence.source_generation.is_none()
                && evidence.origin == Origin::HostRuntime,
            "Linux host probe evidence is stale or has an invalid origin"
        );
        Ok((capability, evidence))
    })
    .collect::<anyhow::Result<Vec<_>>>()?;

    if snapshot.host != HostKind::Linux {
        for (_, evidence) in facts {
            anyhow::ensure!(
                evidence.state == State::NotApplicable
                    && evidence.code == Code::HostCapabilityNotProbed
                    && evidence.observation.is_none(),
                "non-Linux snapshot contains a Linux host probe claim"
            );
        }
        return Ok(());
    }

    for (capability, evidence) in facts {
        validate_state_code(capability, evidence)?;
    }
    typed::validate(snapshot)?;
    Ok(())
}

fn host_fact(snapshot: &MachineSnapshotV1, capability: Capability) -> anyhow::Result<&Evidence> {
    snapshot
        .probe_evidence
        .iter()
        .find(|item| {
            item.prerequisite.domain == Domain::Host
                && item.prerequisite.platform == snapshot.host
                && item.prerequisite.capability == capability
                && item.prerequisite.subject == Subject::None
        })
        .ok_or_else(|| anyhow::anyhow!("Linux host probe evidence is missing for {capability:?}"))
}

fn validate_state_code(capability: Capability, evidence: &Evidence) -> anyhow::Result<()> {
    let valid = matches!(
        (capability, evidence.state, evidence.code),
        (
            Capability::LinuxRootlessRecipe,
            State::Ready,
            Code::LinuxRootlessRecipeCandidate
        ) | (
            Capability::LinuxRootlessRecipe,
            State::Missing,
            Code::LinuxHostCheckMissing
        ) | (
            Capability::LinuxRootlessRecipe,
            State::Unknown,
            Code::LinuxProbeInaccessible | Code::LinuxRootlessRecipeUnverified,
        ) | (
            Capability::LinuxRootlessRecipe,
            State::Failed,
            Code::LinuxProbeMalformed
        ) | (
            Capability::LinuxKernel
                | Capability::LinuxUserNamespaces
                | Capability::LinuxSubuid
                | Capability::LinuxSubgid
                | Capability::LinuxRootlessHelpers
                | Capability::LinuxCgroups,
            State::Ready,
            Code::LinuxHostCheckReady,
        ) | (
            Capability::LinuxKernel
                | Capability::LinuxUserNamespaces
                | Capability::LinuxSubuid
                | Capability::LinuxSubgid
                | Capability::LinuxRootlessHelpers
                | Capability::LinuxCgroups,
            State::Missing,
            Code::LinuxHostCheckMissing,
        ) | (
            Capability::LinuxKernel
                | Capability::LinuxUserNamespaces
                | Capability::LinuxSubuid
                | Capability::LinuxSubgid
                | Capability::LinuxRootlessHelpers
                | Capability::LinuxCgroups,
            State::Unknown,
            Code::LinuxProbeInaccessible | Code::HostCapabilityNotProbed,
        ) | (
            Capability::LinuxKernel
                | Capability::LinuxUserNamespaces
                | Capability::LinuxSubuid
                | Capability::LinuxSubgid
                | Capability::LinuxRootlessHelpers
                | Capability::LinuxCgroups,
            State::Failed,
            Code::LinuxProbeMalformed,
        ) | (
            Capability::LinuxUserNamespaces | Capability::LinuxCgroups,
            State::Unsupported,
            Code::LinuxProbeUnsupported,
        ) | (
            Capability::LinuxUidMappings,
            State::Ready,
            Code::LinuxUidMappingsConfigured
        ) | (
            Capability::LinuxUidMappings,
            State::Missing,
            Code::LinuxUidMappingsMissing
        ) | (
            Capability::LinuxUidMappings,
            State::Unknown,
            Code::HostCapabilityNotProbed | Code::LinuxProbeInaccessible,
        ) | (
            Capability::LinuxUidMappings,
            State::Failed,
            Code::LinuxProbeMalformed
        ) | (
            Capability::LinuxServiceManager,
            State::Ready,
            Code::LinuxServiceManagerDetected
        ) | (
            Capability::LinuxServiceManager,
            State::Missing,
            Code::LinuxHostCheckMissing
        ) | (
            Capability::LinuxServiceManager,
            State::Unknown,
            Code::HostCapabilityNotProbed | Code::LinuxProbeInaccessible,
        ) | (
            Capability::LinuxServiceManager,
            State::Failed,
            Code::LinuxProbeMalformed
        ) | (
            Capability::LinuxServiceManager,
            State::Unsupported,
            Code::LinuxProbeUnsupported
        ) | (
            Capability::LinuxPackageManager,
            State::Ready,
            Code::LinuxPackageManagerDetected
        ) | (
            Capability::LinuxPackageManager,
            State::Missing,
            Code::LinuxPackageManagerMissing
        ) | (
            Capability::LinuxPackageManager,
            State::Unknown,
            Code::HostCapabilityNotProbed | Code::LinuxProbeInaccessible,
        ) | (
            Capability::LinuxPackageManager,
            State::Failed,
            Code::LinuxProbeMalformed
        )
    );
    anyhow::ensure!(
        valid,
        "Linux host probe state/code mismatch for {capability:?}"
    );
    Ok(())
}
