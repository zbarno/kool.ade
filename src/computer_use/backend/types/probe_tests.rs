use std::time::Duration;

use super::{ComputerBackendProbeV1, ComputerHostPlatformV1};
use crate::computer_use::{
    ComputerHypervisorV1 as Hypervisor, ComputerRemoteWorkerTrustV1, ComputerTargetV1 as Target,
    guest_image::ComputerGuestArchitectureV1 as Architecture,
};

fn make_probe(platform: ComputerHostPlatformV1) -> ComputerBackendProbeV1 {
    ComputerBackendProbeV1 {
        host_platform: Some(platform),
        hypervisor_accessible: true,
        qemu_available: true,
        delegated_resource_controls: true,
        max_vcpus: 8,
        max_memory_bytes: 8 * 1024 * 1024 * 1024,
        max_overlay_bytes: 16 * 1024 * 1024 * 1024,
        max_processes: 64,
        parallel_slots_per_machine: 1,
        boot_timeout: Duration::from_secs(120),
    }
}

fn local(hypervisor: Hypervisor) -> Target {
    Target::LocalVm {
        machine_id: "00000000-0000-4000-8000-000000000001".into(),
        hypervisor,
    }
}

#[test]
fn verified_host_target_selects_the_native_signed_guest_architecture() {
    let cases = [
        (
            ComputerHostPlatformV1::MacOsIntel,
            Hypervisor::QemuHvf,
            Architecture::X86_64,
        ),
        (
            ComputerHostPlatformV1::MacOsAppleSilicon,
            Hypervisor::QemuHvf,
            Architecture::Aarch64,
        ),
        (
            ComputerHostPlatformV1::WindowsX64,
            Hypervisor::QemuWhpx,
            Architecture::X86_64,
        ),
        (
            ComputerHostPlatformV1::LinuxX86_64,
            Hypervisor::QemuKvm,
            Architecture::X86_64,
        ),
    ];

    for (platform, hypervisor, architecture) in cases {
        let probe = make_probe(platform);
        let target = local(hypervisor);
        assert!(
            probe.supports_target(&target),
            "{platform:?} should support its candidate"
        );
        assert_eq!(probe.guest_architecture_for(&target), Some(architecture));
    }
}

#[test]
fn remote_target_requires_remote_probe_and_stays_x86_64() {
    let target = Target::RemoteVmWorker {
        worker_id: "worker-1".into(),
        machine_id: "00000000-0000-4000-8000-000000000001".into(),
        trust: ComputerRemoteWorkerTrustV1 {
            ssh_host_key_fingerprint: String::new(),
            worker_attestation_key_id: String::new(),
            worker_attestation_verifying_key_hex: String::new(),
            transport_protocol_version: 0,
            guest_protocol_version: 0,
        },
    };
    let probe = make_probe(ComputerHostPlatformV1::LinuxX86_64);
    assert_eq!(probe.guest_architecture_for(&target), None);

    let mut probe = make_probe(ComputerHostPlatformV1::LinuxX86_64);
    probe.host_platform = None;
    assert!(probe.supports_target(&target));
    assert_eq!(
        probe.guest_architecture_for(&target),
        Some(Architecture::X86_64)
    );
}

#[test]
fn mismatched_platforms_and_unverified_controls_fail_closed() {
    let combinations = [
        (ComputerHostPlatformV1::MacOsIntel, Hypervisor::QemuKvm),
        (ComputerHostPlatformV1::WindowsX64, Hypervisor::QemuHvf),
        (ComputerHostPlatformV1::LinuxX86_64, Hypervisor::QemuWhpx),
        (ComputerHostPlatformV1::LinuxAarch64, Hypervisor::QemuKvm),
    ];
    for (platform, hypervisor) in combinations {
        let probe = make_probe(platform);
        assert!(!probe.supports_target(&local(hypervisor)));
    }

    let target = local(Hypervisor::QemuHvf);
    let mut probe = make_probe(ComputerHostPlatformV1::MacOsAppleSilicon);
    probe.hypervisor_accessible = false;
    assert_eq!(probe.guest_architecture_for(&target), None);
    let mut probe = make_probe(ComputerHostPlatformV1::MacOsAppleSilicon);
    probe.qemu_available = false;
    assert_eq!(probe.guest_architecture_for(&target), None);
    let mut probe = make_probe(ComputerHostPlatformV1::MacOsAppleSilicon);
    probe.delegated_resource_controls = false;
    assert_eq!(probe.guest_architecture_for(&target), None);
    let mut probe = make_probe(ComputerHostPlatformV1::MacOsAppleSilicon);
    probe.max_memory_bytes = 0;
    assert_eq!(probe.guest_architecture_for(&target), None);
}
