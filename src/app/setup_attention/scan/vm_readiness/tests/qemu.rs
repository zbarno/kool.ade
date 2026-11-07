use super::super::{Check, QemuCapabilities, VmFacts, findings_for};
use crate::app::setup_attention::catalog::FindingStatus;

#[test]
fn cpu_flags_and_qemu_capability_do_not_prove_kvm_or_compatibility() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Unknown,
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    let kvm = cards
        .iter()
        .find(|card| card.id == "setup:computer:kvm")
        .unwrap();
    assert_eq!(kvm.status, FindingStatus::Unverified);
    let qemu = cards
        .iter()
        .find(|card| card.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.status, FindingStatus::Unverified);
    assert!(qemu.explanation.contains("minimum supported version"));
    assert!(qemu.explanation.contains("9.2.0"));
}

#[test]
fn old_qemu_never_claims_compatibility_without_a_backend_minimum_contract() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 2.0.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    let qemu = cards
        .iter()
        .find(|card| card.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.status, FindingStatus::Unverified);
    assert!(
        qemu.explanation
            .contains("Detected version: QEMU emulator version 2.0.0")
    );
}

#[test]
fn permission_denied_kvm_and_qemu_without_acceleration_are_actionable() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::PermissionDenied,
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: false,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    for id in ["setup:computer:kvm", "setup:computer:qemu"] {
        assert_eq!(
            cards.iter().find(|card| card.id == id).unwrap().status,
            FindingStatus::NeedsAttention
        );
    }
}
