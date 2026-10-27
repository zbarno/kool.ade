use super::*;
use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostKind, ImageArchitectureV1 as Architecture,
    PrerequisiteCapability as Capability, PrerequisiteDomain, PrerequisiteId, PrerequisiteSubject,
    ProbeEvidenceV1, ProbeObservationV1 as Observation, WindowsEditionV1 as Edition,
    WindowsProductV1 as Product,
};

fn evidence(capability: Capability, state: EvidenceState) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: HostKind::Windows,
            capability,
            subject: PrerequisiteSubject::None,
        },
        state,
        code: EvidenceCode::HostCapabilityNotProbed,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: 1,
        source_generation: None,
        observed_at_unix_ms: 1,
        observation: None,
    }
}

fn unsupported_host(product: Product, architecture: Architecture) -> ProbeEvidenceV1 {
    let mut item = evidence(Capability::HostEdition, EvidenceState::Unsupported);
    item.observation = Some(Observation::WindowsHost {
        product,
        edition: Edition::Pro,
        build_number: if product == Product::Windows10 {
            19_041
        } else {
            22_000
        },
        architecture,
    });
    item
}

#[test]
fn windows_blockers_create_specific_setup_tasks() {
    let evidence = [
        evidence(Capability::HostEdition, EvidenceState::Unsupported),
        evidence(Capability::WindowsWslFeature, EvidenceState::Missing),
        evidence(Capability::WindowsVmPlatformFeature, EvidenceState::Missing),
        evidence(Capability::WindowsWslKernel, EvidenceState::Missing),
        evidence(Capability::WindowsReboot, EvidenceState::Missing),
        evidence(Capability::WindowsWslPolicy, EvidenceState::Blocked),
        evidence(Capability::Virtualization, EvidenceState::Missing),
    ];
    let mut findings = Vec::new();
    append_findings(&mut findings, HostKind::Windows, &evidence);
    let ids = findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<Vec<_>>();
    for id in [
        "setup:windows:host-version",
        "setup:windows:wsl-policy",
        "setup:windows:wsl-features",
        "setup:windows:reboot",
        "setup:windows:virtualization",
        "setup:windows:wsl-kernel",
    ] {
        assert!(ids.contains(&id), "missing finding {id}");
    }
}

#[test]
fn passing_readiness_probes_still_require_native_windows_validation() {
    let evidence = [
        evidence(Capability::HostEdition, EvidenceState::Ready),
        evidence(Capability::WindowsWslFeature, EvidenceState::Ready),
        evidence(Capability::WindowsVmPlatformFeature, EvidenceState::Ready),
        evidence(Capability::WindowsWslKernel, EvidenceState::Ready),
        evidence(Capability::WindowsReboot, EvidenceState::Ready),
        evidence(Capability::WindowsWslPolicy, EvidenceState::Ready),
        evidence(Capability::Virtualization, EvidenceState::Ready),
    ];
    let mut findings = Vec::new();
    append_findings(&mut findings, HostKind::Windows, &evidence);
    let finding = findings
        .iter()
        .find(|finding| finding.id == "setup:windows:wsl2-native-validation")
        .expect("native validation task is present");
    assert_eq!(finding.status, FindingStatus::Unsupported);
    assert!(finding.next_step.contains("sensitive-file denial"));
}

#[test]
fn windows_findings_are_not_emitted_for_other_platforms() {
    let mut findings = Vec::new();
    append_findings(&mut findings, HostKind::Linux, &[]);
    assert!(findings.is_empty());
}

#[test]
fn windows_10_remains_unsupported_until_separate_native_acceptance() {
    let mut findings = Vec::new();
    append_findings(
        &mut findings,
        HostKind::Windows,
        &[unsupported_host(Product::Windows10, Architecture::Amd64)],
    );
    let finding = findings
        .iter()
        .find(|finding| finding.id == "setup:windows:host-version")
        .expect("Windows 10 support gate is present");
    assert_eq!(finding.status, FindingStatus::Unsupported);
    assert!(finding.explanation.contains("separate native acceptance"));
}

#[test]
fn arm64_remains_unsupported_until_separate_native_acceptance() {
    let mut findings = Vec::new();
    append_findings(
        &mut findings,
        HostKind::Windows,
        &[unsupported_host(Product::Windows11, Architecture::Arm64)],
    );
    let finding = findings
        .iter()
        .find(|finding| finding.id == "setup:windows:host-version")
        .expect("ARM64 support gate is present");
    assert_eq!(finding.status, FindingStatus::Unsupported);
    assert!(finding.explanation.contains("Windows ARM64"));
}
