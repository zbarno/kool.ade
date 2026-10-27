use crate::app::setup_attention::catalog::{
    EvidenceState as State, HostKind, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject,
};

use super::super::super::MachineSnapshotV1;

pub(super) fn validate(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    let get = |capability| {
        snapshot
            .probe_evidence
            .iter()
            .find(|item| {
                item.prerequisite.domain == Domain::Host
                    && item.prerequisite.platform == snapshot.host
                    && item.prerequisite.capability == capability
                    && item.prerequisite.subject == Subject::None
            })
            .map(|item| item.state)
    };
    let capabilities = [
        Capability::WindowsWslFeature,
        Capability::WindowsVmPlatformFeature,
        Capability::WindowsWslKernel,
        Capability::WindowsReboot,
        Capability::WindowsWslPolicy,
    ];
    let observed = capabilities.map(get);
    if observed.iter().all(Option::is_none) {
        return Ok(());
    }
    anyhow::ensure!(
        observed.iter().all(Option::is_some),
        "Windows host evidence is incomplete"
    );
    if snapshot.host != HostKind::Windows {
        for state in observed.into_iter().flatten() {
            anyhow::ensure!(
                state == State::NotApplicable,
                "Windows-only probes must be not applicable on other hosts"
            );
        }
        return Ok(());
    }
    let edition = get(Capability::HostEdition)
        .ok_or_else(|| anyhow::anyhow!("Windows edition evidence is missing"))?;
    let policy = get(Capability::WindowsWslPolicy)
        .ok_or_else(|| anyhow::anyhow!("Windows WSL policy evidence is missing"))?;
    let virtualization = get(Capability::Virtualization)
        .ok_or_else(|| anyhow::anyhow!("Windows virtualization evidence is missing"))?;
    let wsl2 = if policy == State::Blocked {
        State::Blocked
    } else if edition == State::Unsupported {
        State::Unsupported
    } else if virtualization == State::Missing {
        State::Blocked
    } else {
        let mut states = vec![edition, virtualization, policy];
        states.extend(
            capabilities
                .iter()
                .map(|capability| {
                    get(*capability).ok_or_else(|| {
                        anyhow::anyhow!("Windows host evidence is missing: {capability:?}")
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        if states.contains(&State::Missing) {
            State::Missing
        } else if states.iter().all(|state| *state == State::Ready) {
            State::Ready
        } else {
            State::Unknown
        }
    };
    anyhow::ensure!(
        snapshot.capability_matrix.windows_wsl2 == wsl2,
        "WSL2 capability matrix conflicts with typed Windows evidence"
    );
    Ok(())
}
