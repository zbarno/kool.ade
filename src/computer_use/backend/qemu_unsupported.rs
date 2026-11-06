use std::{path::PathBuf, time::Duration};

use ed25519_dalek::VerifyingKey;

use crate::computer_use::{
    ComputerBackendErrorV1 as Error, ComputerBackendEventV1 as Event,
    ComputerBackendProbeV1 as Probe, ComputerGuestChannelV1, ComputerGuestReadinessV1,
    ComputerMachineHandleV1 as Handle, ComputerMachineRecordV1 as Record,
    ComputerResourceLimitsV1 as Limits, ComputerSessionV1, guest_image::GuestImageOverlayV1,
};

use super::types::ComputerBackend;

/// Build-compatible fail-closed implementation for hosts without Linux KVM.
pub struct LocalKvmComputerBackend;

impl LocalKvmComputerBackend {
    pub fn new(
        _runtime_root: PathBuf,
        _overlay_root: PathBuf,
        _delegated_cgroup_root: PathBuf,
        _host_machine_id: String,
        _controller_key_id: String,
        _controller_key: VerifyingKey,
        _limits: Limits,
    ) -> Result<Self, Error> {
        Ok(Self)
    }
}

impl ComputerBackend for LocalKvmComputerBackend {
    fn probe(&self) -> Result<Probe, Error> {
        Err(Error::UnsupportedHost)
    }

    fn prepare(
        &self,
        _session: &ComputerSessionV1,
        _overlay: GuestImageOverlayV1,
        _now_unix_ms: u64,
    ) -> Result<(Handle, Event), Error> {
        Err(Error::UnsupportedHost)
    }

    fn recover(&self) -> Result<Vec<Event>, Error> {
        Err(Error::UnsupportedHost)
    }

    fn boot(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn wait_ready(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _timeout: Duration,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn connect(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<ComputerGuestChannelV1, Error> {
        Err(Error::UnsupportedHost)
    }

    fn mark_guest_ready(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _readiness: ComputerGuestReadinessV1,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn suspend(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn resume(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn snapshot(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Record, Error> {
        Err(Error::UnsupportedHost)
    }

    fn cancel(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn stop(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }

    fn collect(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Record, Error> {
        Err(Error::UnsupportedHost)
    }

    fn release(
        &self,
        _session: &ComputerSessionV1,
        _handle: &Handle,
        _now: u64,
    ) -> Result<Event, Error> {
        Err(Error::UnsupportedHost)
    }
}
