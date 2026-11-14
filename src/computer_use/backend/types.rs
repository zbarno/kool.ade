use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::Path;
use std::time::Duration;

#[cfg(target_os = "linux")]
use super::ComputerBackendErrorV1;

mod backend;
mod guest_channel;

pub use backend::ComputerBackend;
pub use guest_channel::{ComputerGuestChannelV1, ComputerGuestReadinessV1};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerHostPlatformV1 {
    LinuxX86_64,
    LinuxAarch64,
    MacOsIntel,
    MacOsAppleSilicon,
    WindowsX64,
    WindowsArm64,
    Unsupported,
}

impl ComputerHostPlatformV1 {
    pub fn current() -> Self {
        Self::from_os_arch(std::env::consts::OS, std::env::consts::ARCH)
    }

    fn from_os_arch(os: &str, architecture: &str) -> Self {
        match (os, architecture) {
            ("linux", "x86_64") => Self::LinuxX86_64,
            ("linux", "aarch64") => Self::LinuxAarch64,
            ("macos", "x86_64") => Self::MacOsIntel,
            ("macos", "aarch64") => Self::MacOsAppleSilicon,
            ("windows", "x86_64") => Self::WindowsX64,
            ("windows", "aarch64") => Self::WindowsArm64,
            _ => Self::Unsupported,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerBackendProbeV1 {
    /// `None` means the adapter cannot authoritatively report a remote host.
    pub host_platform: Option<ComputerHostPlatformV1>,
    pub linux_host: bool,
    pub kvm_accessible: bool,
    pub qemu_available: bool,
    pub delegated_resource_controls: bool,
    pub max_vcpus: u16,
    pub max_memory_bytes: u64,
    pub max_overlay_bytes: u64,
    pub max_processes: u16,
    pub parallel_slots_per_machine: u16,
    pub boot_timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerResourceLimitsV1 {
    pub max_vcpus: u16,
    pub max_memory_bytes: u64,
    pub max_overlay_bytes: u64,
    pub max_processes: u16,
    pub parallel_slots_per_machine: u16,
    pub boot_timeout: Duration,
    pub stop_timeout: Duration,
}

impl Default for ComputerResourceLimitsV1 {
    fn default() -> Self {
        Self {
            max_vcpus: 8,
            max_memory_bytes: 16 * 1024 * 1024 * 1024,
            max_overlay_bytes: 32 * 1024 * 1024 * 1024,
            // Linux pids.max counts QEMU worker threads as well as processes.
            // Desktop guests can exceed 32 QEMU tasks during boot and disk I/O.
            max_processes: 128,
            parallel_slots_per_machine: 1,
            boot_timeout: Duration::from_secs(120),
            stop_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerMachineHandleV1 {
    pub(super) session_id: String,
    pub(super) lease_id: String,
    pub(super) lease_generation: u64,
    pub(super) machine_id: String,
}

impl ComputerMachineHandleV1 {
    pub(crate) fn from_remote(
        session_id: String,
        lease_id: String,
        lease_generation: u64,
        machine_id: String,
    ) -> Self {
        Self {
            session_id,
            lease_id,
            lease_generation,
            machine_id,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn lease_generation(&self) -> u64 {
        self.lease_generation
    }

    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }
    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerMachineRecordV1 {
    pub session_id: String,
    pub machine_id: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub image_digest: String,
    pub policy_fingerprint: String,
    pub state: ComputerMachineStateV1,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub vcpus: u16,
    pub overlay_bytes: Option<u64>,
    pub exit_code: Option<i32>,
}

/// Host-validated input for the private, local Computer Use display proxy.
/// No socket path or free-form VNC command can cross the controller boundary.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerDisplayInputV1 {
    Pointer { x: u16, y: u16, buttons: u8 },
    Key { keysym: u32, pressed: bool },
    Text { text: String },
}

/// A bounded image returned by the controller-owned local display proxy.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerDisplayFrameV1 {
    pub width: u16,
    pub height: u16,
    pub png: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerMachineStateV1 {
    Prepared,
    Booting,
    Connected,
    Suspended,
    TimedOut,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerBackendEventV1 {
    pub sequence: u64,
    pub session_id: String,
    pub lease_generation: u64,
    pub kind: ComputerBackendEventKindV1,
    pub boot_fingerprint: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerBackendEventKindV1 {
    Prepared,
    Booted,
    Recovered,
    GuestReady,
    TimedOut,
    Suspended,
    Resumed,
    Cancelled,
    Stopped,
    Collected,
    Released,
}

#[cfg(target_os = "linux")]
pub(super) fn validate_runtime_root(path: &Path) -> Result<(), ComputerBackendErrorV1> {
    std::fs::create_dir_all(path).map_err(|_| ComputerBackendErrorV1::Io)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| ComputerBackendErrorV1::UnsafeRuntimeDirectory)?;
    if !metadata.file_type().is_dir() {
        return Err(ComputerBackendErrorV1::UnsafeRuntimeDirectory);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid {
            return Err(ComputerBackendErrorV1::UnsafeRuntimeDirectory);
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| ComputerBackendErrorV1::Io)?;
        let tightened = std::fs::symlink_metadata(path)
            .map_err(|_| ComputerBackendErrorV1::UnsafeRuntimeDirectory)?;
        if tightened.permissions().mode() & 0o077 != 0 {
            return Err(ComputerBackendErrorV1::UnsafeRuntimeDirectory);
        }
    }
    Ok(())
}

#[cfg(test)]
mod host_platform_tests {
    use super::ComputerHostPlatformV1 as Platform;

    #[test]
    fn platform_detection_keeps_os_architecture_pairs_distinct() {
        assert_eq!(
            Platform::from_os_arch("macos", "x86_64"),
            Platform::MacOsIntel
        );
        assert_eq!(
            Platform::from_os_arch("macos", "aarch64"),
            Platform::MacOsAppleSilicon
        );
        assert_eq!(
            Platform::from_os_arch("windows", "x86_64"),
            Platform::WindowsX64
        );
        assert_eq!(
            Platform::from_os_arch("linux", "aarch64"),
            Platform::LinuxAarch64
        );
        assert_eq!(
            Platform::from_os_arch("freebsd", "x86_64"),
            Platform::Unsupported
        );
    }
}
