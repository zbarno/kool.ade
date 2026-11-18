use std::{io, path::Path, process::Command, time::Duration};

use super::MacOsVmProcessV1;
use crate::computer_use::{
    backend::ComputerHostPlatformV1, macos_backend::MacOsHvfReadiness,
    macos_backend::probe_macos_hvf,
};

const MAX_GUEST_MEMORY_BYTES: u64 = 16 * 1024 * 1024 * 1024;

impl MacOsVmProcessV1 {
    /// Start the executable selected by a fixed macOS launch plan. The
    /// executable is supplied by the trusted runtime resolver; requiring an
    /// absolute path and an exact plan name prevents ambient PATH lookup and
    /// cross-architecture binary substitution.
    pub(crate) fn spawn_qemu(
        plan: &super::super::qemu_plan::MacOsQemuLaunchPlanV1,
        executable: &Path,
        cpu_time_limit: Duration,
    ) -> io::Result<Self> {
        let host_platform = ComputerHostPlatformV1::current();
        let readiness = probe_macos_hvf();
        validate_qemu_launch_inputs(plan, executable, host_platform, readiness)?;
        let mut command = Command::new(executable);
        command.args(&plan.arguments);
        Self::spawn_with_qmp(
            command,
            plan.memory_bytes,
            plan.disk_bytes,
            cpu_time_limit,
            plan.qmp_socket.clone(),
        )
    }
}

fn validate_qemu_launch_inputs(
    plan: &super::super::qemu_plan::MacOsQemuLaunchPlanV1,
    executable: &Path,
    host_platform: ComputerHostPlatformV1,
    readiness: MacOsHvfReadiness,
) -> io::Result<()> {
    if !executable.is_absolute()
        || executable.file_name().and_then(|name| name.to_str()) != Some(plan.executable)
        || plan.platform != host_platform
        || !readiness.ready_for_adapter
        || readiness.blocker.is_some()
        || !readiness.release_enabled
        || !plan.release_enabled
        || !(512 * 1024 * 1024..=MAX_GUEST_MEMORY_BYTES).contains(&plan.memory_bytes)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "macOS QEMU runtime does not match its bounded launch plan",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use crate::computer_use::{
        backend::ComputerHostPlatformV1,
        macos_backend::qemu_plan::{MacOsQemuLaunchRequestV1, build_macos_qemu_launch_plan},
    };

    use super::{MAX_GUEST_MEMORY_BYTES, MacOsVmProcessV1, validate_qemu_launch_inputs};
    use crate::computer_use::macos_backend::MacOsHvfReadiness;

    #[test]
    fn qemu_launch_requires_matching_absolute_architecture_binary_and_bounded_ram() {
        let platform = match ComputerHostPlatformV1::current() {
            ComputerHostPlatformV1::MacOsIntel | ComputerHostPlatformV1::MacOsAppleSilicon => {
                ComputerHostPlatformV1::current()
            }
            _ => ComputerHostPlatformV1::MacOsAppleSilicon,
        };
        let plan = build_macos_qemu_launch_plan(
            platform,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                disk_bytes: 2 * 1024 * 1024 * 1024,
                overlay_virtual_size_bytes: 2 * 1024 * 1024 * 1024,
                overlay: Path::new("/private/session/disk.qcow2"),
                control_socket: Path::new("/private/session/control.sock"),
                qmp_socket: Path::new("/private/session/qmp.sock"),
                display_socket: Path::new("/private/session/display.sock"),
                boot_config: Path::new("/private/session/boot.json"),
                app_helper_config: Path::new("/private/session/helper.json"),
            },
        )
        .expect("valid native macOS plan");

        let executable_name = plan.executable;
        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &plan,
                Path::new(executable_name),
                Duration::from_secs(60),
            )
            .is_err()
        );
        let gated = match MacOsVmProcessV1::spawn_qemu(
            &plan,
            &Path::new("/app/bin").join(executable_name),
            Duration::from_secs(60),
        ) {
            Err(error) => error,
            Ok(_) => panic!("a closed release gate must prevent process creation"),
        };
        assert_eq!(gated.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &plan,
                &Path::new("/app/bin").join(if executable_name == "qemu-system-aarch64" {
                    "qemu-system-x86_64"
                } else {
                    "qemu-system-aarch64"
                }),
                Duration::from_secs(60),
            )
            .is_err()
        );

        let mut over_budget = plan;
        over_budget.memory_bytes = MAX_GUEST_MEMORY_BYTES + 1;
        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &over_budget,
                &Path::new("/app/bin").join(executable_name),
                Duration::from_secs(60),
            )
            .is_err()
        );
    }

    #[test]
    fn native_hypervisor_and_platform_gates_are_rechecked_before_qemu_spawn() {
        let platform = ComputerHostPlatformV1::MacOsAppleSilicon;
        let mut plan = build_macos_qemu_launch_plan(
            platform,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                disk_bytes: 2 * 1024 * 1024 * 1024,
                overlay_virtual_size_bytes: 2 * 1024 * 1024 * 1024,
                overlay: Path::new("/private/session/disk.qcow2"),
                control_socket: Path::new("/private/session/control.sock"),
                qmp_socket: Path::new("/private/session/qmp.sock"),
                display_socket: Path::new("/private/session/display.sock"),
                boot_config: Path::new("/private/session/boot.json"),
                app_helper_config: Path::new("/private/session/helper.json"),
            },
        )
        .expect("valid Apple silicon plan");
        plan.release_enabled = true;
        let executable = Path::new("/app/bin/qemu-system-aarch64");
        let ready_but_closed = MacOsHvfReadiness {
            ready_for_adapter: true,
            blocker: None,
            release_enabled: false,
        };
        assert!(
            validate_qemu_launch_inputs(&plan, executable, platform, ready_but_closed).is_err()
        );

        let hypothetically_open = MacOsHvfReadiness {
            ready_for_adapter: true,
            blocker: None,
            release_enabled: true,
        };
        assert!(
            validate_qemu_launch_inputs(
                &plan,
                executable,
                ComputerHostPlatformV1::MacOsIntel,
                hypothetically_open,
            )
            .is_err()
        );
    }
}
