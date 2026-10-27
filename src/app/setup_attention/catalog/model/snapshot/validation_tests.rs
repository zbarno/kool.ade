use super::{
    ImageArchitectureV1, ImageProfileV1, PrerequisiteSubject, ProbeEvidenceV1,
    claims::windows_observation_is_consistent, requires_macos_release,
    rootless_smoke_subject_matches, valid_macos_version,
};
use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, PrerequisiteCapability, PrerequisiteDomain,
    PrerequisiteId, ProbeObservationV1, WindowsEditionV1, WindowsKernelVersionV1, WindowsProductV1,
};

#[test]
fn macos_release_versions_are_bounded_numeric_components() {
    assert!(valid_macos_version("15.4.1"));
    assert!(!valid_macos_version(""));
    assert!(!valid_macos_version("15..1"));
    assert!(!valid_macos_version("15.4-beta"));
    assert!(!valid_macos_version(&"1".repeat(33)));
}

#[test]
fn rootless_smoke_receipt_subject_must_be_general_and_match_its_platform() {
    assert!(rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/amd64",
    ));
    assert!(rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Arm64,
        },
        "linux/arm64",
    ));
    assert!(!rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::Rust,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/amd64",
    ));
    assert!(!rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/arm64",
    ));
}

#[test]
fn windows_probe_observations_must_match_state_and_code() {
    let mut item = ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: super::HostKind::Windows,
            capability: PrerequisiteCapability::WindowsWslKernel,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::WindowsWslKernelDetected,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: 1,
        source_generation: None,
        observed_at_unix_ms: 1,
        observation: Some(ProbeObservationV1::WindowsWslKernel {
            version: Some(WindowsKernelVersionV1 {
                major: 5,
                minor: 15,
                patch: 153,
                build: Some(1),
            }),
        }),
    };
    assert!(windows_observation_is_consistent(&item));
    item.state = EvidenceState::Missing;
    assert!(!windows_observation_is_consistent(&item));
}

#[test]
fn windows_host_observation_rejects_unsupported_claims_marked_ready() {
    let item = ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: super::HostKind::Windows,
            capability: PrerequisiteCapability::HostEdition,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::WindowsHostDetected,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: 1,
        source_generation: None,
        observed_at_unix_ms: 1,
        observation: Some(ProbeObservationV1::WindowsHost {
            product: WindowsProductV1::Windows10,
            edition: WindowsEditionV1::Pro,
            build_number: 19_041,
            architecture: ImageArchitectureV1::Amd64,
        }),
    };
    assert!(!windows_observation_is_consistent(&item));
}

#[test]
fn ready_windows_host_edition_does_not_require_macos_release_evidence() {
    let item = ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: super::HostKind::Windows,
            capability: PrerequisiteCapability::HostEdition,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::WindowsHostDetected,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: 1,
        source_generation: None,
        observed_at_unix_ms: 1,
        observation: Some(ProbeObservationV1::WindowsHost {
            product: WindowsProductV1::Windows11,
            edition: WindowsEditionV1::Pro,
            build_number: 22_000,
            architecture: ImageArchitectureV1::Amd64,
        }),
    };
    assert!(!requires_macos_release(&item));
}

#[test]
fn windows_arm64_observation_cannot_claim_the_x64_support_baseline() {
    let item = ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: super::HostKind::Windows,
            capability: PrerequisiteCapability::HostEdition,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::WindowsHostDetected,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: 1,
        source_generation: None,
        observed_at_unix_ms: 1,
        observation: Some(ProbeObservationV1::WindowsHost {
            product: WindowsProductV1::Windows11,
            edition: WindowsEditionV1::Pro,
            build_number: 22_000,
            architecture: ImageArchitectureV1::Arm64,
        }),
    };
    assert!(!windows_observation_is_consistent(&item));
}
