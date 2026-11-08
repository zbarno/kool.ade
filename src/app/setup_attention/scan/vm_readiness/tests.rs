use super::*;

#[path = "tests/qemu.rs"]
mod qemu;

#[test]
fn vm_blockers_have_stable_cards_and_dependency_order() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Missing,
        qemu: Check::Missing,
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    assert_eq!(
        cards
            .iter()
            .map(|card| card.id.as_str())
            .collect::<Vec<_>>(),
        [
            "setup:computer:image:amd64",
            "setup:computer:kvm",
            "setup:computer:network",
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
    assert!(qemu.prerequisites.is_empty());
    assert_eq!(qemu.status, FindingStatus::NeedsAttention);
    let kvm = cards
        .iter()
        .find(|card| card.id == "setup:computer:kvm")
        .unwrap();
    assert!(kvm.explanation.contains("CPU flags alone do not prove KVM"));
    let network = cards
        .iter()
        .find(|card| card.id == "setup:computer:network")
        .unwrap();
    assert_eq!(
        network.prerequisites,
        [
            "setup:computer:image:amd64",
            "setup:computer:kvm",
            "setup:computer:qemu",
            "setup:computer:storage"
        ]
    );
    assert_eq!(network.status, FindingStatus::Unsupported);
    for id in [
        "setup:computer:network",
        "setup:computer:viewer",
        "setup:computer:personal-encryption",
    ] {
        let card = cards.iter().find(|card| card.id == id).unwrap();
        assert!(
            card.prerequisites
                .contains(&"setup:computer:storage".into())
        );
        assert!(card.prerequisites.contains(&"setup:computer:kvm".into()));
    }
}

#[test]
fn verified_kvm_is_not_kept_as_a_downstream_task_dependency() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
        qemu: Check::Unknown,
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Ready(()),
    });
    assert!(!cards.iter().any(|card| card.id == "setup:computer:kvm"));
    for id in [
        "setup:computer:network",
        "setup:computer:viewer",
        "setup:computer:personal-encryption",
    ] {
        let card = cards.iter().find(|card| card.id == id).unwrap();
        assert!(!card.prerequisites.contains(&"setup:computer:kvm".into()));
    }
}

#[test]
fn successful_kvm_recheck_closes_only_its_checked_domain_idempotently() {
    let mut domains = vec!["git".into()];
    append_checked_domains(&mut domains, true, true);
    append_checked_domains(&mut domains, true, true);
    assert_eq!(domains, ["computer:guest-image", "computer:kvm", "git"]);
}

#[test]
fn promoted_image_stays_unverified_without_local_state_but_unimplemented_domains_stay_unsupported()
{
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    assert_eq!(
        cards
            .iter()
            .find(|card| card.id == "setup:computer:storage")
            .unwrap()
            .status,
        FindingStatus::Unverified,
        "storage must remain blocked without its owning component evidence"
    );
    assert_eq!(
        cards
            .iter()
            .find(|card| card.id == "setup:computer:image:amd64")
            .unwrap()
            .status,
        FindingStatus::Unverified,
        "the signed bundled image still needs a verified local floor and cache"
    );
    for id in [
        "setup:computer:network",
        "setup:computer:viewer",
        "setup:computer:personal-encryption",
    ] {
        assert_eq!(
            cards.iter().find(|card| card.id == id).unwrap().status,
            FindingStatus::Unsupported,
            "{id} must reflect a missing implementation in this build"
        );
    }
}

#[test]
fn known_resource_shortfall_blocks_storage_without_claiming_overlay_readiness() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: resources::ResourceCapacity {
            available_memory_bytes: Some(
                crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1.memory_bytes,
            ),
            available_storage_bytes: Some(
                crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1.disk_bytes - 1,
            ),
            ..Default::default()
        },
        nested_virtualization: Check::Unknown,
    });
    let storage = cards
        .iter()
        .find(|card| card.id == "setup:computer:storage")
        .unwrap();
    assert_eq!(storage.status, FindingStatus::NeedsAttention);
    assert!(storage.explanation.contains("storage is below"));
}

#[test]
fn unknown_distribution_never_gets_a_guessed_qemu_install_recipe() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Unknown,
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Missing,
        qemu: Check::Missing,
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    let qemu = cards
        .iter()
        .find(|card| card.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.status, FindingStatus::NeedsAttention);
    assert!(qemu.next_step.contains("unknown distribution"));
    assert!(qemu.next_step.contains("no package command is guessed"));
}

#[test]
fn nested_virtualization_is_unsupported_even_when_x86_64_is_detected() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Ready(()),
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Unsupported,
        qemu: Check::Ready(QemuCapabilities {
            version: "QEMU emulator version 9.2.0".into(),
            kvm_acceleration: true,
            qemu_image_tool: Check::Ready(()),
        }),
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unsupported,
    });
    let kvm = cards
        .iter()
        .find(|card| card.id == "setup:computer:kvm")
        .unwrap();
    assert_eq!(kvm.status, FindingStatus::Unsupported);
    assert!(kvm.explanation.contains("nested virtualization"));
}

#[test]
fn missing_user_namespaces_block_only_the_disposable_computer_path() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Ready(()),
        user_namespaces: Check::Missing,
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
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
    assert_eq!(kvm.status, FindingStatus::NeedsAttention);
    assert!(kvm.explanation.contains("user namespaces are disabled"));
}

#[test]
fn unsupported_platform_classifies_kvm_as_unsupported() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Unsupported,
        kernel: Check::Unsupported,
        user_namespaces: Check::Unsupported,
        cpu_virtualization: Check::Unsupported,
        kvm: Check::Unsupported,
        qemu: Check::Unsupported,
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Unknown,
    });
    let kvm = cards
        .iter()
        .find(|card| card.id == "setup:computer:kvm")
        .expect("unsupported platforms retain the stable KVM task card");
    assert_eq!(kvm.status, FindingStatus::Unsupported);
    assert!(kvm.explanation.contains("x86_64 Linux"));
    let image = cards
        .iter()
        .find(|card| card.id == "setup:computer:image:amd64")
        .unwrap();
    assert_eq!(image.status, FindingStatus::Unsupported);
    let qemu = cards
        .iter()
        .find(|card| card.id == "setup:computer:qemu")
        .unwrap();
    assert_eq!(qemu.status, FindingStatus::Unsupported);
}

#[test]
fn arm64_linux_does_not_probe_x86_qemu_as_a_supported_capability() {
    let facts = facts::collect(
        HostKind::Linux,
        ImageArchitectureV1::Arm64,
        &[],
        1,
        Default::default(),
    );
    assert_eq!(facts.architecture, Check::Unsupported);
    assert_eq!(facts.kvm, Check::Unsupported);
    assert_eq!(facts.qemu, Check::Unsupported);
    let cards = findings_for(facts);
    for id in ["setup:computer:kvm", "setup:computer:qemu"] {
        assert_eq!(
            cards.iter().find(|card| card.id == id).unwrap().status,
            FindingStatus::Unsupported,
            "{id} must reflect the unsupported host architecture"
        );
    }
}

#[test]
fn unresolved_kernel_or_user_namespace_keeps_kvm_finding_active() {
    let cards = findings_for(VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Unknown,
        user_namespaces: Check::Unknown,
        cpu_virtualization: Check::Ready(()),
        kvm: Check::Ready(()),
        qemu: Check::Unknown,
        qemu_launcher_containment: Check::Ready(()),
        resources: Default::default(),
        nested_virtualization: Check::Ready(()),
    });
    assert!(cards.iter().any(|card| card.id == "setup:computer:kvm"));
}
