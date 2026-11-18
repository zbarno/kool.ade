//! Deterministic WHPX command and resource planning. The plan is not enabled
//! until executable trust, storage/image integration, and AF_UNIX client
//! verification pass.

#[cfg(windows)]
use std::io;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use super::WindowsVmJobLimits;

mod path;
#[cfg(windows)]
mod session;
#[cfg(test)]
#[path = "launch_plan/tests.rs"]
mod tests;

const MIB: u64 = 1024 * 1024;
const QEMU_JOB_OVERHEAD_BYTES: u64 = 1024 * MIB;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowsLaunchBlocker {
    InvalidSession,
    UnsupportedTarget,
    UnsupportedSessionClass,
    NetworkNotDisabled,
    InvalidResourceBudget,
    InvalidOverlay,
    UnsafePath,
    InvalidHostCapacity,
}

#[derive(Clone, Debug)]
pub(crate) struct WindowsWpxLaunchPlan {
    executable: PathBuf,
    args: Vec<OsString>,
    job_limits: WindowsVmJobLimits,
    control_socket_path: PathBuf,
    vcpus: u16,
    memory_bytes: u64,
    disk_bytes: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct WindowsWpxLaunchInput<'a> {
    pub(crate) qemu_executable: &'a Path,
    pub(crate) overlay: &'a Path,
    pub(crate) overlay_root: &'a Path,
    pub(crate) overlay_virtual_size_bytes: u64,
    pub(crate) boot_config: &'a Path,
    pub(crate) session_directory: &'a Path,
    pub(crate) guest_control_socket: &'a Path,
    pub(crate) session_id: &'a str,
    pub(crate) cpu_millis: u32,
    pub(crate) memory_bytes: u64,
    pub(crate) disk_bytes: u64,
    pub(crate) disposable_session: bool,
    pub(crate) network_disabled: bool,
}

impl WindowsWpxLaunchPlan {
    /// Builds only from controller-validated session and image objects. The
    /// caller must revalidate session authority, image trust, and overlay
    /// contents immediately before constructing this plan.
    pub(crate) fn build(
        input: WindowsWpxLaunchInput<'_>,
        host_logical_processors: u16,
    ) -> Result<Self, WindowsLaunchBlocker> {
        if !path::canonical_session_id(input.session_id) {
            return Err(WindowsLaunchBlocker::InvalidSession);
        }
        if !input.disposable_session {
            return Err(WindowsLaunchBlocker::UnsupportedSessionClass);
        }
        if !input.network_disabled {
            return Err(WindowsLaunchBlocker::NetworkNotDisabled);
        }
        if !(100..=64_000).contains(&input.cpu_millis)
            || !(256 * MIB..=128 * 1024 * MIB).contains(&input.memory_bytes)
            || !(256 * MIB..=512 * 1024 * MIB).contains(&input.disk_bytes)
        {
            return Err(WindowsLaunchBlocker::InvalidResourceBudget);
        }
        if host_logical_processors == 0 {
            return Err(WindowsLaunchBlocker::InvalidHostCapacity);
        }
        if input.overlay.parent() != Some(input.overlay_root)
            || input.overlay_virtual_size_bytes != input.disk_bytes
        {
            return Err(WindowsLaunchBlocker::InvalidOverlay);
        }
        if input.boot_config.parent() != Some(input.session_directory) {
            return Err(WindowsLaunchBlocker::UnsafePath);
        }
        if input.guest_control_socket.file_name()
            != Some(std::ffi::OsStr::new("guest-control-v1.sock"))
            || input
                .guest_control_socket
                .parent()
                .and_then(Path::file_name)
                != Some(std::ffi::OsStr::new(input.session_id))
        {
            return Err(WindowsLaunchBlocker::UnsafePath);
        }
        for path in [
            input.qemu_executable,
            input.overlay,
            input.boot_config,
            input.guest_control_socket,
        ] {
            path::validate_qemu_path(path)?;
        }

        let requested_vcpus = input.cpu_millis.div_ceil(1_000).max(1);
        let vcpus = requested_vcpus.min(u32::from(host_logical_processors)) as u16;
        let cpu_rate = (u64::from(input.cpu_millis) * 10)
            .div_ceil(u64::from(host_logical_processors))
            .clamp(1, 10_000) as u32;
        let process_memory = input
            .memory_bytes
            .checked_add(QEMU_JOB_OVERHEAD_BYTES)
            .ok_or(WindowsLaunchBlocker::InvalidResourceBudget)?;
        let job_limits = WindowsVmJobLimits::new(process_memory, cpu_rate, 1)
            .ok_or(WindowsLaunchBlocker::InvalidResourceBudget)?;

        let control_socket_path = input.guest_control_socket.to_path_buf();
        let args = path::qemu_args(
            input.overlay,
            input.boot_config,
            &control_socket_path,
            vcpus,
            input.memory_bytes,
        )?;
        Ok(Self {
            executable: input.qemu_executable.to_path_buf(),
            args,
            job_limits,
            control_socket_path,
            vcpus,
            memory_bytes: input.memory_bytes,
            disk_bytes: input.disk_bytes,
        })
    }

    pub(crate) fn arguments(&self) -> &[OsString] {
        &self.args
    }

    pub(crate) fn control_socket_path(&self) -> &Path {
        &self.control_socket_path
    }

    pub(crate) fn resource_summary(&self) -> (u16, u64, u64) {
        (self.vcpus, self.memory_bytes, self.disk_bytes)
    }

    pub(crate) fn job_resource_limits(&self) -> (usize, u32, u32) {
        (
            self.job_limits.process_memory_bytes,
            self.job_limits.cpu_rate_hundredths_percent,
            self.job_limits.active_processes,
        )
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .args(&self.args)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command
    }

    #[cfg(windows)]
    pub(crate) fn start(&self) -> io::Result<super::process::WindowsVmProcessJob> {
        super::transport::require_qemu_af_unix_evidence()?;
        super::process::WindowsVmProcessJob::spawn(self.command(), self.job_limits)
    }
}
