//! Fixed QEMU/HVF launch planning for native-architecture macOS guests.
//!
//! This module only produces a deterministic command plan. It deliberately
//! does not spawn QEMU: trusted executable verification, the complete session
//! lifecycle, private transport ownership, and the host release gate remain
//! incomplete.

use std::{ffi::OsString, path::Path};

use crate::computer_use::backend::ComputerHostPlatformV1;

const MIB: u64 = 1024 * 1024;
pub(super) const MAX_VCPUS: u32 = 16;
const MIN_MEMORY_MIB: u64 = 512;
const MAX_MEMORY_MIB: u64 = 16 * 1024;
const MAX_UNIX_SOCKET_PATH_BYTES: usize = 103;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacOsQemuLaunchPlanV1 {
    pub(crate) platform: ComputerHostPlatformV1,
    pub(crate) executable: &'static str,
    pub(crate) arguments: Vec<OsString>,
    pub(crate) qmp_socket: std::path::PathBuf,
    /// Guest RAM encoded into `arguments`; process launch derives its resource
    /// bound from this value rather than accepting a second caller value.
    pub(crate) memory_bytes: u64,
    /// Maximum session-authorized qcow2 overlay size. This becomes the QEMU
    /// process RLIMIT_FSIZE and caps individual child-created files.
    pub(crate) disk_bytes: u64,
    pub(crate) release_enabled: bool,
}

pub(super) struct MacOsQemuLaunchRequestV1<'a> {
    pub(crate) vcpus: u32,
    pub(crate) memory_bytes: u64,
    pub(crate) disk_bytes: u64,
    pub(crate) overlay_virtual_size_bytes: u64,
    pub(crate) overlay: &'a Path,
    pub(crate) control_socket: &'a Path,
    pub(crate) qmp_socket: &'a Path,
    pub(crate) display_socket: &'a Path,
    pub(crate) boot_config: &'a Path,
    pub(crate) app_helper_config: &'a Path,
}

/// Build an architecture-specific, offline QEMU/HVF command plan from
/// controller-owned resource values and app-owned paths. The caller must
/// source `disk_bytes` from the verified session policy and the overlay size
/// from the verified image overlay. The plan cannot run while the release gate
/// is closed or until the remaining macOS lifecycle controls are implemented.
pub(super) fn build_macos_qemu_launch_plan(
    platform: ComputerHostPlatformV1,
    request: MacOsQemuLaunchRequestV1<'_>,
) -> Option<MacOsQemuLaunchPlanV1> {
    let MacOsQemuLaunchRequestV1 {
        vcpus,
        memory_bytes,
        disk_bytes,
        overlay_virtual_size_bytes,
        overlay,
        control_socket,
        qmp_socket,
        display_socket,
        boot_config,
        app_helper_config,
    } = request;
    let (executable, machine, cpu, display_device) = match platform {
        ComputerHostPlatformV1::MacOsIntel => {
            ("qemu-system-x86_64", "q35,accel=hvf", "host", "virtio-vga")
        }
        ComputerHostPlatformV1::MacOsAppleSilicon => (
            "qemu-system-aarch64",
            "virt,accel=hvf",
            "host",
            "virtio-gpu-pci",
        ),
        _ => return None,
    };
    if vcpus == 0 || vcpus > MAX_VCPUS {
        return None;
    }
    // Round down so QEMU never receives a memory value above the signed budget.
    let memory_mib = memory_bytes / MIB;
    if !(MIN_MEMORY_MIB..=MAX_MEMORY_MIB).contains(&memory_mib) {
        return None;
    }
    if !(256 * MIB..=512 * 1024 * MIB).contains(&disk_bytes)
        || overlay_virtual_size_bytes == 0
        || overlay_virtual_size_bytes > disk_bytes
    {
        return None;
    }
    for path in [overlay, boot_config, app_helper_config] {
        if !safe_qemu_path(path) {
            return None;
        }
    }
    for path in [control_socket, qmp_socket, display_socket] {
        if !safe_qemu_path(path)
            || path.as_os_str().as_encoded_bytes().len() > MAX_UNIX_SOCKET_PATH_BYTES
        {
            return None;
        }
    }

    let vcpus = vcpus.to_string();
    let memory = format!("{memory_mib}M");
    let drive = format!(
        "if=none,id=guestdisk,file={},format=qcow2,cache=none,aio=threads,discard=unmap",
        overlay.display()
    );
    let control = format!(
        "socket,id=guestcontrol,path={},server=on,wait=off",
        control_socket.display()
    );
    let qmp = format!("unix:{},server=on,wait=off", qmp_socket.display());
    let vnc = format!("unix:{},share=allow-exclusive", display_socket.display());
    let boot = format!(
        "name=opt/org.koolade.computer-use/boot-config,file={}",
        boot_config.display()
    );
    let helper = format!(
        "name=opt/org.koolade.computer-use/app-helper-config,file={}",
        app_helper_config.display()
    );
    let raw = [
        "-no-user-config",
        "-nodefaults",
        "-machine",
        machine,
        "-cpu",
        cpu,
        "-smp",
        &vcpus,
        "-m",
        &memory,
        "-drive",
        &drive,
        "-device",
        "virtio-blk-pci,drive=guestdisk",
        "-device",
        display_device,
        "-display",
        "none",
        "-vnc",
        &vnc,
        "-qmp",
        &qmp,
        "-nic",
        "none",
        "-monitor",
        "none",
        "-serial",
        "none",
        "-parallel",
        "none",
        "-chardev",
        &control,
        "-device",
        "virtio-serial-pci",
        "-device",
        "virtserialport,chardev=guestcontrol,name=org.koolade.computer-control.v1",
        "-fw_cfg",
        &boot,
        "-fw_cfg",
        &helper,
        "-no-reboot",
    ];
    Some(MacOsQemuLaunchPlanV1 {
        platform,
        executable,
        arguments: raw.iter().map(OsString::from).collect(),
        qmp_socket: qmp_socket.to_path_buf(),
        memory_bytes: memory_mib * MIB,
        disk_bytes,
        // Planning a launch never certifies the host or opens its support gate.
        release_enabled: false,
    })
}

pub(super) fn safe_qemu_path(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().as_encoded_bytes().iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b' ' | b'/' | b'_' | b'-' | b'.')
        })
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(platform: ComputerHostPlatformV1) -> MacOsQemuLaunchPlanV1 {
        build_macos_qemu_launch_plan(
            platform,
            MacOsQemuLaunchRequestV1 {
                vcpus: 4,
                memory_bytes: 4 * MIB * 1024,
                disk_bytes: 2 * 1024 * MIB,
                overlay_virtual_size_bytes: 2 * 1024 * MIB,
                overlay: Path::new("/private/session/disk.qcow2"),
                control_socket: Path::new("/private/session/control.sock"),
                qmp_socket: Path::new("/private/session/qmp.sock"),
                display_socket: Path::new("/private/session/display.sock"),
                boot_config: Path::new("/private/session/boot.json"),
                app_helper_config: Path::new("/private/session/helper.json"),
            },
        )
        .expect("valid native plan")
    }

    fn plan_for_disk_budget(
        disk_bytes: u64,
        overlay_virtual_size_bytes: u64,
    ) -> Option<MacOsQemuLaunchPlanV1> {
        build_macos_qemu_launch_plan(
            ComputerHostPlatformV1::MacOsIntel,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 2 * MIB * 1024,
                disk_bytes,
                overlay_virtual_size_bytes,
                overlay: Path::new("/private/session/disk.qcow2"),
                control_socket: Path::new("/private/session/control.sock"),
                qmp_socket: Path::new("/private/session/qmp.sock"),
                display_socket: Path::new("/private/session/display.sock"),
                boot_config: Path::new("/private/session/boot.json"),
                app_helper_config: Path::new("/private/session/helper.json"),
            },
        )
    }

    #[test]
    fn intel_plan_uses_x86_hvf_and_has_no_guest_network() {
        let plan = plan(ComputerHostPlatformV1::MacOsIntel);
        let args = plan.arguments.iter().map(|value| value.to_string_lossy());
        let args = args.collect::<Vec<_>>();
        assert_eq!(plan.executable, "qemu-system-x86_64");
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-machine", "q35,accel=hvf"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-nic", "none"]));
        assert!(
            args.windows(2).any(|pair| {
                pair == ["-qmp", "unix:/private/session/qmp.sock,server=on,wait=off"]
            })
        );
        assert!(!plan.release_enabled);
    }

    #[test]
    fn apple_silicon_plan_uses_aarch64_hvf_machine_and_gpu() {
        let plan = plan(ComputerHostPlatformV1::MacOsAppleSilicon);
        let args = plan.arguments.iter().map(|value| value.to_string_lossy());
        let args = args.collect::<Vec<_>>();
        assert_eq!(plan.executable, "qemu-system-aarch64");
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-machine", "virt,accel=hvf"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-device", "virtio-gpu-pci"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-nic", "none"]));
        assert!(args.windows(2).any(|pair| pair[0] == "-qmp"));
        assert!(!plan.release_enabled);
    }

    #[test]
    fn macos_launch_plans_have_no_network_or_host_directory_shares() {
        for platform in [
            ComputerHostPlatformV1::MacOsIntel,
            ComputerHostPlatformV1::MacOsAppleSilicon,
        ] {
            let plan = plan(platform);
            let args = plan
                .arguments
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();

            assert!(args.windows(2).any(|pair| pair == ["-nic", "none"]));
            for forbidden in ["-net", "-netdev", "-virtfs", "-fsdev", "-smb"] {
                assert!(!args.iter().any(|arg| arg == forbidden), "{forbidden}");
            }
            assert!(!args.iter().any(|arg| arg.contains("hostfwd=")));
            assert!(!args.iter().any(|arg| arg.starts_with("tcp:")));
            assert_eq!(
                args.iter().filter(|arg| arg.as_ref() == "-drive").count(),
                1
            );
            assert!(
                args.iter()
                    .any(|arg| { arg.contains("file=/private/session/disk.qcow2") })
            );
            assert!(args.iter().any(|arg| {
                arg.contains("socket,id=guestcontrol,path=/private/session/control.sock")
            }));
        }
    }

    #[test]
    fn launch_plan_rejects_unsupported_hosts_and_unbounded_inputs() {
        let valid = plan(ComputerHostPlatformV1::MacOsIntel);
        let args_for = |vcpus, memory, overlay: &Path, socket: &Path, qmp: &Path| {
            build_macos_qemu_launch_plan(
                ComputerHostPlatformV1::MacOsIntel,
                MacOsQemuLaunchRequestV1 {
                    vcpus,
                    memory_bytes: memory,
                    disk_bytes: 2 * 1024 * MIB,
                    overlay_virtual_size_bytes: 2 * 1024 * MIB,
                    overlay,
                    control_socket: socket,
                    qmp_socket: qmp,
                    display_socket: Path::new("/private/session/display.sock"),
                    boot_config: Path::new("/private/session/boot.json"),
                    app_helper_config: Path::new("/private/session/helper.json"),
                },
            )
        };
        assert!(!valid.release_enabled);
        let rounded_down = build_macos_qemu_launch_plan(
            ComputerHostPlatformV1::MacOsIntel,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: MIN_MEMORY_MIB * MIB + 1,
                disk_bytes: 2 * 1024 * MIB,
                overlay_virtual_size_bytes: 2 * 1024 * MIB,
                overlay: Path::new("/disk"),
                control_socket: Path::new("/sock"),
                qmp_socket: Path::new("/qmp"),
                display_socket: Path::new("/display"),
                boot_config: Path::new("/boot"),
                app_helper_config: Path::new("/helper"),
            },
        )
        .unwrap();
        assert!(
            rounded_down.arguments.windows(2).any(|pair| {
                pair[0] == "-m" && pair[1] == format!("{MIN_MEMORY_MIB}M").as_str()
            })
        );
        let qmp = Path::new("/private/session/qmp.sock");
        assert!(
            args_for(
                0,
                4 * MIB * 1024,
                Path::new("/disk"),
                Path::new("/sock"),
                qmp
            )
            .is_none()
        );
        assert!(
            args_for(
                17,
                4 * MIB * 1024,
                Path::new("/disk"),
                Path::new("/sock"),
                qmp
            )
            .is_none()
        );
        assert!(args_for(2, 1, Path::new("/disk"), Path::new("/sock"), qmp).is_none());
        assert!(
            args_for(
                2,
                4 * MIB * 1024,
                Path::new("relative"),
                Path::new("/sock"),
                qmp
            )
            .is_none()
        );
        assert!(
            args_for(
                2,
                4 * MIB * 1024,
                Path::new("/disk,server=on"),
                Path::new("/sock"),
                qmp
            )
            .is_none()
        );
        assert!(args_for(
            2,
            4 * MIB * 1024,
            Path::new("/disk"),
            Path::new("/private/session/this-socket-path-is-intentionally-much-longer-than-the-unix-domain-socket-limit-and-must-be-rejected.sock"),
            qmp
        )
        .is_none());
        assert!(
            args_for(
                2,
                4 * MIB * 1024,
                Path::new("/disk"),
                Path::new("/sock"),
                Path::new("/qmp,server=on")
            )
            .is_none()
        );
        assert!(
            build_macos_qemu_launch_plan(
                ComputerHostPlatformV1::LinuxX86_64,
                MacOsQemuLaunchRequestV1 {
                    vcpus: 2,
                    memory_bytes: 4 * MIB * 1024,
                    disk_bytes: 2 * 1024 * MIB,
                    overlay_virtual_size_bytes: 2 * 1024 * MIB,
                    overlay: Path::new("/disk"),
                    control_socket: Path::new("/sock"),
                    qmp_socket: Path::new("/qmp"),
                    display_socket: Path::new("/display"),
                    boot_config: Path::new("/boot"),
                    app_helper_config: Path::new("/helper"),
                },
            )
            .is_none()
        );
    }

    #[test]
    fn launch_plan_binds_overlay_size_to_the_signed_disk_budget() {
        const GIB: u64 = 1024 * MIB;
        assert!(plan_for_disk_budget(0, 0).is_none());
        assert!(plan_for_disk_budget(2 * GIB, 4 * GIB).is_none());
        let plan = plan_for_disk_budget(3 * GIB, 2 * GIB).expect("bounded disk plan");
        assert_eq!(plan.disk_bytes, 3 * GIB);
    }

    #[test]
    fn launch_plan_preserves_standard_macos_application_support_paths() {
        let path = Path::new(
            "/Users/operator/Library/Application Support/koolade/computer-use/macos-vm/v1/session.qcow2",
        );
        let plan = build_macos_qemu_launch_plan(
            ComputerHostPlatformV1::MacOsIntel,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 2 * MIB * 1024,
                disk_bytes: 2 * 1024 * MIB,
                overlay_virtual_size_bytes: 2 * 1024 * MIB,
                overlay: path,
                control_socket: Path::new("/var/folders/u/T/ku/session/control.sock"),
                qmp_socket: Path::new("/var/folders/u/T/ku/session/qmp.sock"),
                display_socket: Path::new("/var/folders/u/T/ku/session/display.sock"),
                boot_config: Path::new(
                    "/Users/operator/Library/Application Support/koolade/session/boot.json",
                ),
                app_helper_config: Path::new(
                    "/Users/operator/Library/Application Support/koolade/session/helper.json",
                ),
            },
        )
        .expect("standard Application Support paths are safe argv values");
        assert!(plan.arguments.iter().any(|arg| {
            arg.to_string_lossy()
                .contains("file=/Users/operator/Library/Application Support/koolade")
        }));
    }
}
