#[cfg(target_os = "linux")]
use std::path::Path;
use std::time::Duration;

use crate::computer_use::{ComputerSessionV1, guest_image::GuestImageOverlayV1};

use super::ComputerBackendErrorV1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerBackendProbeV1 {
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
            max_processes: 32,
            parallel_slots_per_machine: 1,
            boot_timeout: Duration::from_secs(120),
            stop_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerMachineHandleV1 {
    pub(super) session_id: String,
    pub(super) lease_id: String,
    pub(super) lease_generation: u64,
    pub(super) machine_id: String,
}

impl ComputerMachineHandleV1 {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn lease_generation(&self) -> u64 {
        self.lease_generation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerMachineStateV1 {
    Prepared,
    Booting,
    Connected,
    Suspended,
    TimedOut,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerBackendEventV1 {
    pub sequence: u64,
    pub session_id: String,
    pub lease_generation: u64,
    pub kind: ComputerBackendEventKindV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// Host-side byte stream to one app-owned guest serial port. Its I/O methods
/// stay crate-private so task/provider code cannot bypass the typed protocol.
pub struct ComputerGuestChannelV1 {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) session_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) lease_generation: u64,
    #[cfg(unix)]
    _stream: std::os::unix::net::UnixStream,
}

/// Token minted only by the trusted guest protocol verifier after it has
/// authenticated a fresh per-boot transcript and matched the active lease.
pub struct ComputerGuestReadinessV1 {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) session_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) lease_generation: u64,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) guest_boot_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) transcript_sha256: String,
}

impl ComputerGuestReadinessV1 {
    #[allow(dead_code)] // Used by the authenticated guest transport implemented in the dependent issue.
    pub(crate) fn new(
        session_id: String,
        lease_generation: u64,
        guest_boot_id: String,
        transcript_sha256: String,
    ) -> Self {
        Self {
            session_id,
            lease_generation,
            guest_boot_id,
            transcript_sha256,
        }
    }
}

impl ComputerGuestChannelV1 {
    #[cfg(unix)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn new(
        session_id: String,
        lease_generation: u64,
        stream: std::os::unix::net::UnixStream,
    ) -> Self {
        Self {
            session_id,
            lease_generation,
            _stream: stream,
        }
    }

    #[cfg(unix)]
    #[allow(dead_code)] // The typed guest protocol owns all reads and writes on this channel.
    pub(crate) fn io(&mut self) -> &mut std::os::unix::net::UnixStream {
        &mut self._stream
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn lease_generation(&self) -> u64 {
        self.lease_generation
    }
}

/// Typed VM operations. No operation accepts a model supplied path or QEMU argument.
pub trait ComputerBackend: Send + Sync {
    fn probe(&self) -> Result<ComputerBackendProbeV1, ComputerBackendErrorV1>;
    fn prepare(
        &self,
        session: &ComputerSessionV1,
        overlay: GuestImageOverlayV1,
        now_unix_ms: u64,
    ) -> Result<(ComputerMachineHandleV1, ComputerBackendEventV1), ComputerBackendErrorV1>;
    fn recover(&self) -> Result<Vec<ComputerBackendEventV1>, ComputerBackendErrorV1>;
    fn boot(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn wait_ready(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        timeout: Duration,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn connect(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerGuestChannelV1, ComputerBackendErrorV1>;
    fn mark_guest_ready(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        readiness: ComputerGuestReadinessV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn suspend(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn resume(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn snapshot(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerMachineRecordV1, ComputerBackendErrorV1>;
    fn cancel(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn stop(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
    fn collect(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerMachineRecordV1, ComputerBackendErrorV1>;
    fn release(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<ComputerBackendEventV1, ComputerBackendErrorV1>;
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
