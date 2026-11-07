use super::super::types::ComputerBackend;
use super::*;
use crate::computer_use::{
    ComputerBackendProbeV1 as Probe, ComputerGuestChannelV1, ComputerGuestReadinessV1,
};
use std::time::Duration;

mod control;
mod launch;
mod prepare;
mod readiness;

impl ComputerBackend for LocalKvmComputerBackend {
    fn probe(&self) -> Result<Probe, Error> {
        self.probe_impl()
    }
    fn prepare(
        &self,
        session: &ComputerSessionV1,
        overlay: GuestImageOverlayV1,
        now_unix_ms: u64,
    ) -> Result<(Handle, Event), Error> {
        self.prepare_impl(session, overlay, now_unix_ms)
    }
    fn recover(&self) -> Result<Vec<Event>, Error> {
        self.recover_impl()
    }
    fn boot(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.boot_impl(session, handle, now_unix_ms)
    }
    fn wait_ready(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        timeout: Duration,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.wait_ready_impl(session, handle, timeout, now_unix_ms)
    }
    fn connect(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<ComputerGuestChannelV1, Error> {
        self.connect_impl(session, handle, now_unix_ms)
    }
    fn authenticate_guest(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1, Error> {
        LocalKvmComputerBackend::authenticate_guest(self, session, handle, now_unix_ms)
    }
    fn open_guest_action_journal(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<crate::computer_use::guest_protocol::GuestActionJournalV1, Error> {
        LocalKvmComputerBackend::open_guest_action_journal(self, session, handle, now_unix_ms)
    }
    fn mark_guest_ready(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        readiness: ComputerGuestReadinessV1,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.mark_guest_ready_impl(session, handle, readiness, now_unix_ms)
    }
    fn suspend(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.suspend_impl(session, handle, now_unix_ms)
    }
    fn resume(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.resume_impl(session, handle, now_unix_ms)
    }
    fn snapshot(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Record, Error> {
        self.snapshot_impl(session, handle, now_unix_ms)
    }
    fn cancel(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.cancel_impl(session, handle, now_unix_ms)
    }
    fn stop(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.stop_impl(session, handle, now_unix_ms)
    }
    fn collect(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Record, Error> {
        self.collect_impl(session, handle, now_unix_ms)
    }
    fn release(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.release_impl(session, handle, now_unix_ms)
    }
}
