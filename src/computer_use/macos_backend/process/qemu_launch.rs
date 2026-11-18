use std::{io, path::Path, process::Command, time::Duration};

use super::MacOsVmProcessV1;

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
        if !executable.is_absolute()
            || executable.file_name().and_then(|name| name.to_str()) != Some(plan.executable)
            || !plan.release_enabled
            || !(512 * 1024 * 1024..=MAX_GUEST_MEMORY_BYTES).contains(&plan.memory_bytes)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "macOS QEMU runtime does not match its bounded launch plan",
            ));
        }
        let mut command = Command::new(executable);
        command.args(&plan.arguments);
        Self::spawn(command, plan.memory_bytes, plan.disk_bytes, cpu_time_limit)
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use crate::computer_use::{
        backend::ComputerHostPlatformV1,
        macos_backend::qemu_plan::{MacOsQemuLaunchRequestV1, build_macos_qemu_launch_plan},
    };

    use super::{MAX_GUEST_MEMORY_BYTES, MacOsVmProcessV1};

    #[test]
    fn qemu_launch_requires_matching_absolute_architecture_binary_and_bounded_ram() {
        let plan = build_macos_qemu_launch_plan(
            ComputerHostPlatformV1::MacOsAppleSilicon,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                disk_bytes: 2 * 1024 * 1024 * 1024,
                overlay_virtual_size_bytes: 2 * 1024 * 1024 * 1024,
                overlay: Path::new("/private/session/disk.qcow2"),
                control_socket: Path::new("/private/session/control.sock"),
                display_socket: Path::new("/private/session/display.sock"),
                boot_config: Path::new("/private/session/boot.json"),
                app_helper_config: Path::new("/private/session/helper.json"),
            },
        )
        .expect("valid Apple silicon plan");

        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &plan,
                Path::new("qemu-system-aarch64"),
                Duration::from_secs(60),
            )
            .is_err()
        );
        let gated = match MacOsVmProcessV1::spawn_qemu(
            &plan,
            Path::new("/app/bin/qemu-system-aarch64"),
            Duration::from_secs(60),
        ) {
            Err(error) => error,
            Ok(_) => panic!("a closed release gate must prevent process creation"),
        };
        assert_eq!(gated.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &plan,
                Path::new("/app/bin/qemu-system-x86_64"),
                Duration::from_secs(60),
            )
            .is_err()
        );

        let mut over_budget = plan;
        over_budget.memory_bytes = MAX_GUEST_MEMORY_BYTES + 1;
        assert!(
            MacOsVmProcessV1::spawn_qemu(
                &over_budget,
                Path::new("/app/bin/qemu-system-aarch64"),
                Duration::from_secs(60),
            )
            .is_err()
        );
    }
}
