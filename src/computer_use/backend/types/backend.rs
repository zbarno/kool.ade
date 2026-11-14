use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1;
use crate::computer_use::{
    ComputerSessionV1, guest_image::GuestImageOverlayV1, guest_protocol::GuestActionJournalV1,
};

use super::super::ComputerBackendErrorV1;
use super::{
    ComputerBackendEventV1, ComputerBackendProbeV1, ComputerDisplayFrameV1, ComputerDisplayInputV1,
    ComputerGuestChannelV1, ComputerGuestReadinessV1, ComputerMachineHandleV1,
    ComputerMachineRecordV1,
};

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
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn authenticate_guest(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<AuthenticatedGuestChannelV1, ComputerBackendErrorV1>;
    fn open_guest_action_journal(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        now_unix_ms: u64,
    ) -> Result<GuestActionJournalV1, ComputerBackendErrorV1>;
    /// Capture one bounded frame from the private VM display socket. Backends
    /// without an independently verified local display return unsupported.
    fn capture_display_frame(
        &self,
        _session: &ComputerSessionV1,
        _handle: &ComputerMachineHandleV1,
        _now_unix_ms: u64,
    ) -> Result<ComputerDisplayFrameV1, ComputerBackendErrorV1> {
        Err(ComputerBackendErrorV1::UnsupportedHost)
    }
    /// Send one validated local input event through the controller-owned
    /// display proxy. Model/provider paths cannot supply a socket or endpoint.
    fn send_display_input(
        &self,
        _session: &ComputerSessionV1,
        _handle: &ComputerMachineHandleV1,
        _input: &ComputerDisplayInputV1,
        _now_unix_ms: u64,
    ) -> Result<(), ComputerBackendErrorV1> {
        Err(ComputerBackendErrorV1::UnsupportedHost)
    }
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
