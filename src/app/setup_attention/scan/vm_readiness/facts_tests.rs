use super::super::QemuCapabilities;
use super::*;
use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostKind, PrerequisiteCapability,
    PrerequisiteDomain, PrerequisiteId, PrerequisiteSubject, ProbeEvidenceV1, ProbeObservationV1,
};

fn user_namespace_evidence(
    generation: u64,
    state: EvidenceState,
    unprivileged_clone_allowed: Option<bool>,
) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: HostKind::Linux,
            capability: PrerequisiteCapability::LinuxUserNamespaces,
            subject: PrerequisiteSubject::None,
        },
        state,
        code: match state {
            EvidenceState::Ready => EvidenceCode::LinuxHostCheckReady,
            EvidenceState::Missing => EvidenceCode::LinuxHostCheckMissing,
            _ => EvidenceCode::HostCapabilityNotProbed,
        },
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: generation,
        source_generation: None,
        observed_at_unix_ms: 10,
        observation: Some(ProbeObservationV1::LinuxUserNamespaces {
            max_user_namespaces: Some(10_000),
            unprivileged_clone_allowed,
            apparmor_userns_restricted: Some(false),
        }),
    }
}

#[test]
fn user_namespace_support_requires_current_positive_policy_evidence() {
    let current_ready = user_namespace_evidence(4, EvidenceState::Ready, Some(true));
    assert_eq!(
        host_user_namespaces_check(&[current_ready], 4),
        Check::Ready(())
    );

    let stale_ready = user_namespace_evidence(3, EvidenceState::Ready, Some(true));
    assert_eq!(
        host_user_namespaces_check(&[stale_ready], 4),
        Check::Unknown
    );

    let policy_denied = user_namespace_evidence(4, EvidenceState::Ready, Some(false));
    assert_eq!(
        host_user_namespaces_check(&[policy_denied], 4),
        Check::Unknown
    );

    let absent = user_namespace_evidence(4, EvidenceState::Missing, None);
    assert_eq!(host_user_namespaces_check(&[absent], 4), Check::Missing);
}

struct ProbeFixture {
    kvm_error: Option<std::io::ErrorKind>,
    qemu_error: Option<std::io::ErrorKind>,
    qemu_img_error: Option<std::io::ErrorKind>,
    launcher_error: Option<std::io::ErrorKind>,
    qemu_version: Vec<u8>,
    qemu_accelerators: Vec<u8>,
}

impl super::super::VmHostProbe for ProbeFixture {
    fn cpu_virtualization_flags(&self) -> std::io::Result<Option<bool>> {
        Ok(Some(true))
    }

    fn virtualization_present(&self) -> std::io::Result<Option<bool>> {
        Ok(Some(false))
    }

    fn open_kvm(&self) -> std::io::Result<()> {
        match self.kvm_error {
            Some(error) => Err(std::io::Error::from(error)),
            None => Ok(()),
        }
    }

    fn qemu_output(&self, arguments: &[&str]) -> std::io::Result<Option<Vec<u8>>> {
        assert!(
            self.launcher_error.is_none(),
            "QEMU must not be executed when launcher containment fails"
        );
        if let Some(error) = self.qemu_error {
            return Err(std::io::Error::from(error));
        }
        let output = match arguments {
            ["--version"] => self.qemu_version.as_slice(),
            ["-accel", "help"] => self.qemu_accelerators.as_slice(),
            _ => return Err(std::io::Error::other("unexpected QEMU probe")),
        };
        Ok(Some(output.to_vec()))
    }

    fn qemu_image_tool(&self) -> std::io::Result<()> {
        match self.qemu_img_error {
            Some(error) => Err(std::io::Error::from(error)),
            None => Ok(()),
        }
    }

    fn launcher_containment(&self) -> std::io::Result<()> {
        match self.launcher_error {
            Some(error) => Err(std::io::Error::from(error)),
            None => Ok(()),
        }
    }
}

fn readiness_evidence() -> Vec<ProbeEvidenceV1> {
    vec![
        ProbeEvidenceV1 {
            prerequisite: PrerequisiteId {
                domain: PrerequisiteDomain::Host,
                platform: HostKind::Linux,
                capability: PrerequisiteCapability::LinuxKernel,
                subject: PrerequisiteSubject::None,
            },
            state: EvidenceState::Ready,
            code: EvidenceCode::LinuxHostCheckReady,
            origin: EvidenceOrigin::HostRuntime,
            scan_generation: 4,
            source_generation: None,
            observed_at_unix_ms: 10,
            observation: Some(ProbeObservationV1::LinuxKernel {
                version: crate::app::setup_attention::catalog::LinuxKernelVersionV1 {
                    major: 6,
                    minor: 8,
                    patch: 0,
                },
            }),
        },
        user_namespace_evidence(4, EvidenceState::Ready, Some(true)),
    ]
}

#[test]
fn kvm_open_failures_flow_through_production_fact_mapping() {
    for (error, expected_check, expected_status, detail_text, guidance) in [
        (
            std::io::ErrorKind::NotFound,
            Check::Missing,
            FindingStatus::NeedsAttention,
            "CPU flags alone",
            "firmware",
        ),
        (
            std::io::ErrorKind::PermissionDenied,
            Check::PermissionDenied,
            FindingStatus::NeedsAttention,
            "cannot open the KVM device",
            "administrator",
        ),
        (
            std::io::ErrorKind::Other,
            Check::Unknown,
            FindingStatus::Unverified,
            "could not be verified safely",
            "administrator",
        ),
    ] {
        let facts = collect::collect_with(
            HostKind::Linux,
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
            &readiness_evidence(),
            4,
            Default::default(),
            &ProbeFixture {
                kvm_error: Some(error),
                qemu_error: None,
                qemu_img_error: None,
                launcher_error: None,
                qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
                qemu_accelerators: b"tcg\nkvm\n".to_vec(),
            },
        );
        assert_eq!(facts.cpu_virtualization, Check::Ready(()));
        assert_eq!(facts.kvm, expected_check);
        let (status, detail, next_step) = kvm_finding(&facts);
        assert_eq!(status, expected_status);
        assert!(detail.contains(detail_text));
        assert!(next_step.contains(guidance));
        assert!(next_step.contains("rescan"));
    }
}

#[test]
fn old_and_absent_qemu_results_flow_through_production_fact_mapping() {
    let old = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        4,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: None,
            qemu_img_error: None,
            launcher_error: None,
            qemu_version: b"QEMU emulator version 2.0.0\n".to_vec(),
            qemu_accelerators: b"tcg\nkvm\n".to_vec(),
        },
    );
    assert_eq!(old.kvm, Check::Ready(()));
    assert_eq!(
        old.qemu,
        Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 2.0.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        })
    );
    let old_qemu = super::super::findings_for(old)
        .into_iter()
        .find(|item| item.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(old_qemu.status, FindingStatus::Unverified);

    let absent = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        4,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: Some(std::io::ErrorKind::NotFound),
            qemu_img_error: None,
            launcher_error: None,
            qemu_version: Vec::new(),
            qemu_accelerators: b"tcg\nkvm\n".to_vec(),
        },
    );
    assert_eq!(absent.kvm, Check::Ready(()));
    assert_eq!(absent.qemu, Check::Missing);
    let absent_qemu = super::super::findings_for(absent)
        .into_iter()
        .find(|item| item.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(absent_qemu.status, FindingStatus::NeedsAttention);
}

#[test]
fn missing_qemu_img_blocks_overlay_readiness_and_points_to_fixed_package_repair() {
    let facts = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        4,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: None,
            qemu_img_error: Some(std::io::ErrorKind::NotFound),
            launcher_error: None,
            qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
            qemu_accelerators: b"tcg\nkvm\n".to_vec(),
        },
    );
    assert_eq!(
        facts.qemu,
        Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Missing,
        })
    );
    let qemu = super::super::findings_for(facts)
        .into_iter()
        .find(|item| item.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.status, FindingStatus::NeedsAttention);
    assert!(qemu.explanation.contains("QEMU image utility"));
    assert!(qemu.next_step.contains("Fix for me"));
}

#[test]
fn untrusted_or_timed_out_qemu_img_probe_never_reports_overlay_tool_ready() {
    for (error, expected, status) in [
        (
            std::io::ErrorKind::PermissionDenied,
            Check::PermissionDenied,
            FindingStatus::NeedsAttention,
        ),
        (
            std::io::ErrorKind::TimedOut,
            Check::Unknown,
            FindingStatus::Unverified,
        ),
    ] {
        let facts = collect::collect_with(
            HostKind::Linux,
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
            &readiness_evidence(),
            4,
            Default::default(),
            &ProbeFixture {
                kvm_error: None,
                qemu_error: None,
                qemu_img_error: Some(error),
                launcher_error: None,
                qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
                qemu_accelerators: b"tcg\nkvm\n".to_vec(),
            },
        );
        let Check::Ready(qemu) = facts.qemu else {
            panic!("valid system QEMU should remain independently detected");
        };
        assert_eq!(qemu.qemu_image_tool, expected);
        let card = super::super::findings_for(VmFacts {
            qemu: Check::Ready(qemu),
            ..facts
        })
        .into_iter()
        .find(|item| item.id == "setup:computer:qemu")
        .unwrap();
        assert_eq!(card.status, status);
    }
}

#[test]
fn unsafe_qemu_launcher_is_typed_and_blocks_the_qemu_task() {
    let facts = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        4,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: None,
            qemu_img_error: None,
            launcher_error: Some(std::io::ErrorKind::PermissionDenied),
            qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
            qemu_accelerators: b"tcg\nkvm\n".to_vec(),
        },
    );
    assert_eq!(facts.qemu, Check::PermissionDenied);
    let card = super::super::findings_for(facts)
        .into_iter()
        .find(|item| item.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(card.status, FindingStatus::NeedsAttention);
    assert!(card.explanation.contains("trusted system ownership"));
    assert!(
        card.next_step
            .contains("Do not change executable permissions")
    );
}

#[test]
fn malformed_qemu_version_is_unknown_not_detected() {
    for qemu_version in [
        b"QEMU emulator version nightly\n".to_vec(),
        b"QEMU emulator version 9.2.0 \xff\n".to_vec(),
    ] {
        let facts = collect::collect_with(
            HostKind::Linux,
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
            &readiness_evidence(),
            4,
            Default::default(),
            &ProbeFixture {
                kvm_error: None,
                qemu_error: None,
                qemu_img_error: None,
                launcher_error: None,
                qemu_version,
                qemu_accelerators: b"tcg\nkvm\n".to_vec(),
            },
        );
        assert_eq!(facts.qemu, Check::Unknown);
        let qemu = super::super::findings_for(facts)
            .into_iter()
            .find(|item| item.id == "setup:computer:qemu")
            .unwrap();
        assert_eq!(qemu.status, FindingStatus::Unverified);
    }
}

#[test]
fn malformed_qemu_accelerator_output_is_unknown_not_detected() {
    let facts = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        4,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: None,
            qemu_img_error: None,
            launcher_error: None,
            qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
            qemu_accelerators: b"tcg\nkvm\n\xff".to_vec(),
        },
    );
    assert_eq!(facts.qemu, Check::Unknown);
}

#[test]
fn stale_host_observations_after_rescan_do_not_authorize_kvm() {
    let facts = collect::collect_with(
        HostKind::Linux,
        crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
        &readiness_evidence(),
        5,
        Default::default(),
        &ProbeFixture {
            kvm_error: None,
            qemu_error: None,
            qemu_img_error: None,
            launcher_error: None,
            qemu_version: b"QEMU emulator version 9.2.0\n".to_vec(),
            qemu_accelerators: b"tcg\nkvm\n".to_vec(),
        },
    );
    assert_eq!(facts.kvm, Check::Ready(()));
    assert_eq!(facts.kernel, Check::Unknown);
    assert_eq!(facts.user_namespaces, Check::Unknown);
    assert_eq!(kvm_finding(&facts).0, FindingStatus::Unverified);
}
