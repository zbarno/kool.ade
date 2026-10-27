use crate::app::setup_attention::catalog::{
    EvidenceState, HostKind, ImageArchitectureV1 as Architecture,
    PrerequisiteCapability as Capability, PrerequisiteDomain, ProbeEvidenceV1,
    ProbeObservationV1 as Observation, WindowsProductV1 as Product,
};

use super::{Finding, FindingStatus};

#[cfg(test)]
mod tests;

pub(super) fn append_findings(
    findings: &mut Vec<Finding>,
    host: HostKind,
    evidence: &[ProbeEvidenceV1],
) {
    if host != HostKind::Windows {
        return;
    }
    let state = |capability| {
        evidence
            .iter()
            .find(|item| {
                item.prerequisite.domain == PrerequisiteDomain::Host
                    && item.prerequisite.capability == capability
            })
            .map(|item| item.state)
            .unwrap_or(EvidenceState::Unknown)
    };
    let host_edition = evidence.iter().find(|item| {
        item.prerequisite.domain == PrerequisiteDomain::Host
            && item.prerequisite.capability == Capability::HostEdition
    });
    match host_edition
        .map(|item| item.state)
        .unwrap_or(EvidenceState::Unknown)
    {
        EvidenceState::Unsupported => {
            let host_observation = host_edition.and_then(|item| item.observation.as_ref());
            let windows10 = matches!(
                host_observation,
                Some(Observation::WindowsHost {
                    product: Product::Windows10,
                    ..
                })
            );
            let arm64 = matches!(
                host_observation,
                Some(Observation::WindowsHost {
                    architecture: Architecture::Arm64,
                    ..
                })
            );
            let finding = if windows10 {
                Finding::unverified(
                    "setup:windows:host-version",
                    "Windows 10 setup awaits native validation",
                    "WSL2 readiness alone does not establish supported Windows 10 execution; separate native acceptance evidence is required.",
                    "Use Windows 11 x64 until Windows 10 native end-to-end acceptance is complete.",
                )
            } else if arm64 {
                Finding::unverified(
                    "setup:windows:host-version",
                    "Windows ARM64 setup awaits native validation",
                    "WSL2 readiness alone does not establish supported Windows ARM64 execution; separate native acceptance evidence is required.",
                    "Use Windows 11 x64 until Windows ARM64 native end-to-end acceptance is complete.",
                )
            } else {
                Finding::action(
                    "setup:windows:host-version",
                    "Use a supported Windows host",
                    "This Windows edition, build, or architecture does not meet the supported WSL2 baseline.",
                    "Use a supported Windows 11 x64 edition and build, then retry the readiness scan.",
                )
            };
            findings.push(Finding {
                status: FindingStatus::Unsupported,
                ..finding
            });
        }
        EvidenceState::Unknown | EvidenceState::Failed => findings.push(Finding::unverified(
            "setup:windows:host-version-check",
            "Verify Windows edition and build",
            "Kool.ad/e could not confirm the Windows edition and build needed for WSL2.",
            "Check Windows edition and build information, then retry the readiness scan.",
        )),
        _ => {}
    }
    match state(Capability::WindowsWslPolicy) {
        EvidenceState::Blocked => findings.push(Finding::action(
            "setup:windows:wsl-policy",
            "Allow WSL by policy",
            "A Windows policy currently disables WSL.",
            "Ask your administrator to enable the Windows WSL policy, then retry.",
        )),
        EvidenceState::Unknown | EvidenceState::Failed => findings.push(Finding::unverified(
            "setup:windows:wsl-policy-check",
            "Verify WSL policy",
            "Kool.ad/e could not verify whether device policy permits WSL.",
            "Review the device's WSL policy with its administrator, then retry.",
        )),
        _ => {}
    }
    if state(Capability::WindowsWslFeature) == EvidenceState::Missing
        || state(Capability::WindowsVmPlatformFeature) == EvidenceState::Missing
    {
        findings.push(Finding::action(
            "setup:windows:wsl-features",
            "Enable WSL2 Windows features",
            "Windows Subsystem for Linux and Virtual Machine Platform must both be enabled.",
            "Enable both optional Windows features, then restart Windows if prompted.",
        ));
    }
    match state(Capability::WindowsReboot) {
        EvidenceState::Missing => findings.push(Finding::action(
            "setup:windows:reboot",
            "Restart Windows to finish setup",
            "Windows reports a pending restart required by feature or update changes.",
            "Save your work and restart Windows, then retry the readiness scan.",
        )),
        EvidenceState::Unknown | EvidenceState::Failed => findings.push(Finding::unverified(
            "setup:windows:reboot-check",
            "Verify Windows restart status",
            "Kool.ad/e could not confirm whether Windows needs to restart.",
            "Check Windows Update and optional feature status, then retry.",
        )),
        _ => {}
    }
    if state(Capability::Virtualization) == EvidenceState::Missing {
        findings.push(Finding::action(
            "setup:windows:virtualization",
            "Enable virtualization for WSL2",
            "Hardware virtualization is not available to Windows.",
            "Enable virtualization in firmware or the parent hypervisor settings, then retry.",
        ));
    }
    match state(Capability::WindowsWslKernel) {
        EvidenceState::Missing => findings.push(Finding::action(
            "setup:windows:wsl-kernel",
            "Install or update the WSL2 kernel",
            "Windows did not report an installed WSL2 Linux kernel.",
            "Run the official WSL update flow, then retry the readiness scan.",
        )),
        EvidenceState::Unknown | EvidenceState::Failed => findings.push(Finding::unverified(
            "setup:windows:wsl-kernel-check",
            "Verify the WSL2 kernel",
            "Kool.ad/e could not confirm the installed WSL2 Linux kernel version.",
            "Run the official WSL status and update flow, then retry.",
        )),
        _ => {}
    }
    let readiness = [
        Capability::HostEdition,
        Capability::WindowsWslFeature,
        Capability::WindowsVmPlatformFeature,
        Capability::WindowsWslKernel,
        Capability::WindowsReboot,
        Capability::WindowsWslPolicy,
        Capability::Virtualization,
    ]
    .into_iter()
    .all(|capability| state(capability) == EvidenceState::Ready);
    if readiness {
        findings.push(Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:windows:wsl2-native-validation",
                "Validate WSL2 on Windows",
                "The Windows readiness probes passed; native Windows execution compatibility still requires end-to-end validation.",
                "Validate a clean Windows setup, Docker Desktop Linux backend, task execution, cancellation, and sensitive-file denial before claiming native support.",
            )
        });
    }
}
