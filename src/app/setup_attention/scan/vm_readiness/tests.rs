use super::*;

#[test]
fn vm_blockers_have_stable_cards_and_dependency_order() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        kvm: Check::Missing,
        qemu: Check::Missing,
    });
    assert_eq!(
        cards
            .iter()
            .map(|card| card.id.as_str())
            .collect::<Vec<_>>(),
        [
            "setup:computer:image:amd64",
            "setup:computer:kvm",
            "setup:computer:personal-encryption",
            "setup:computer:qemu",
            "setup:computer:storage",
            "setup:computer:viewer"
        ]
    );
    let qemu = cards
        .iter()
        .find(|card| card.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.prerequisites, ["setup:computer:kvm"]);
    assert_eq!(qemu.status, FindingStatus::NeedsAttention);
}

#[test]
fn successful_kvm_recheck_closes_only_its_checked_domain_idempotently() {
    let mut domains = vec!["git".into()];
    append_checked_domains(&mut domains, true);
    append_checked_domains(&mut domains, true);
    assert_eq!(domains, ["computer:kvm", "git"]);
}

#[test]
fn cpu_flags_and_qemu_capability_do_not_prove_kvm_or_compatibility() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        kvm: Check::Unknown,
        qemu: Check::Ready(QemuCapabilities {
            kvm_acceleration: true,
        }),
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
}

#[test]
fn permission_denied_kvm_and_qemu_without_acceleration_are_actionable() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        kvm: Check::PermissionDenied,
        qemu: Check::Ready(QemuCapabilities {
            kvm_acceleration: false,
        }),
    });
    for id in ["setup:computer:kvm", "setup:computer:qemu"] {
        assert_eq!(
            cards.iter().find(|card| card.id == id).unwrap().status,
            FindingStatus::NeedsAttention
        );
    }
}

#[test]
fn nested_virtualization_is_unsupported_even_when_x86_64_is_detected() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        kvm: Check::Unsupported,
        qemu: Check::Ready(QemuCapabilities {
            kvm_acceleration: true,
        }),
    });
    let kvm = cards
        .iter()
        .find(|card| card.id == "setup:computer:kvm")
        .unwrap();
    assert_eq!(kvm.status, FindingStatus::Unsupported);
    assert!(kvm.explanation.contains("nested virtualization"));
}

#[test]
fn non_linux_is_unsupported_without_claiming_readiness() {
    let cards = findings_for(VmFacts {
        architecture: Check::Unsupported,
        kvm: Check::Unsupported,
        qemu: Check::Unsupported,
    });
    assert!(cards.iter().all(|card| {
        card.status == FindingStatus::Unsupported || card.status == FindingStatus::Unverified
    }));
    assert!(
        cards
            .iter()
            .any(|card| card.id == "setup:computer:image:amd64")
    );
}
