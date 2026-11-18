use std::time::Duration;

use crate::computer_use::{
    ComputerBackend, ComputerBackendErrorV1 as Error, ComputerBackendEventV1 as Event,
    ComputerBackendProbeV1 as Probe, ComputerDisplayFrameV1, ComputerDisplayInputV1,
    ComputerGuestChannelV1, ComputerGuestReadinessV1, ComputerMachineHandleV1 as Handle,
    ComputerMachineRecordV1 as Record, ComputerSessionV1,
    guest_image::GuestImageOverlayV1,
    guest_protocol::{AuthenticatedGuestChannelV1, GuestActionJournalV1},
};

use super::WindowsComputerBackend;

impl ComputerBackend for WindowsComputerBackend {
    fn probe(&self) -> Result<Probe, Error> {
        Ok(self.probe_for())
    }

    fn prepare(
        &self,
        _session: &ComputerSessionV1,
        _overlay: GuestImageOverlayV1,
        _now_unix_ms: u64,
    ) -> Result<(Handle, Event), Error> {
        Err(self.unavailable_error())
    }

    fn recover(&self) -> Result<Vec<Event>, Error> {
        Err(self.unavailable_error())
    }

    fn boot(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn wait_ready(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _timeout: Duration,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn connect(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<ComputerGuestChannelV1, Error> {
        Err(self.unavailable_error())
    }

    fn authenticate_guest(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<AuthenticatedGuestChannelV1, Error> {
        Err(self.unavailable_error())
    }

    fn open_guest_action_journal(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<GuestActionJournalV1, Error> {
        Err(self.unavailable_error())
    }

    fn capture_display_frame(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<ComputerDisplayFrameV1, Error> {
        Err(self.unavailable_error())
    }

    fn send_display_input(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _input: &ComputerDisplayInputV1,
        _now_unix_ms: u64,
    ) -> Result<(), Error> {
        Err(self.unavailable_error())
    }

    fn mark_guest_ready(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _readiness: ComputerGuestReadinessV1,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn suspend(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn resume(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn snapshot(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Record, Error> {
        Err(self.unavailable_error())
    }

    fn cancel(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn stop(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }

    fn collect(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Record, Error> {
        Err(self.unavailable_error())
    }

    fn release(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now_unix_ms: u64,
    ) -> Result<Event, Error> {
        Err(self.unavailable_error())
    }
}
