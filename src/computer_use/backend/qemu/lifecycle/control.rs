use super::super::*;
use crate::computer_use::ComputerSessionClassV1;

impl LocalKvmComputerBackend {
    pub(super) fn suspend_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if !matches!(machine.owner.state.as_str(), "booting" | "connected") {
            return Err(Error::InvalidTransition);
        }
        if machine
            .supervision
            .load(std::sync::atomic::Ordering::Acquire)
            >= 3
        {
            return Err(Error::QemuTimedOut);
        }
        if let Some(gateway) = machine.network_gateway.as_ref() {
            gateway.pause();
        }
        process::signal_owned(&machine.owner, libc::SIGSTOP)?;
        machine.owner.state = "suspended".to_owned();
        self.event(machine, EventKind::Suspended)
    }

    pub(super) fn resume_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "suspended" {
            return Err(Error::InvalidTransition);
        }
        if machine
            .supervision
            .load(std::sync::atomic::Ordering::Acquire)
            >= 3
        {
            return Err(Error::QemuTimedOut);
        }
        process::signal_owned(&machine.owner, libc::SIGCONT)?;
        if let Some(gateway) = machine.network_gateway.as_ref() {
            gateway.resume();
        }
        machine.owner.state = if machine
            .supervision
            .load(std::sync::atomic::Ordering::Acquire)
            == 1
        {
            "connected".to_owned()
        } else {
            "booting".to_owned()
        };
        self.event(machine, EventKind::Resumed)
    }

    pub(super) fn snapshot_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Record, Error> {
        let machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if session.session_class == ComputerSessionClassV1::Disposable {
            return Err(Error::SnapshotNotAllowed);
        }
        Ok(Self::record(machine, session))
    }

    pub(super) fn cancel_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        self.validate_signed_session_allow_expired(session, now_unix_ms)?;
        if session.identity.session_id != handle.session_id
            || session.lease.lease_id != handle.lease_id
            || target_machine_id(session, self.remote_worker_id.as_deref())? != self.host_machine_id
        {
            return Err(Error::StaleLease);
        }
        let mut machines = self.live.lock().map_err(|_| Error::Io)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.owner_id != session.identity.user_id
            || machine.owner.machine_id != self.host_machine_id
            || machine.owner.lease_id != session.lease.lease_id
            || session.lease.generation < machine.owner.lease_generation
            || session.cancellation_generation <= machine.owner.cancellation_generation
        {
            return Err(Error::StaleLease);
        }
        machine.owner.cancellation_generation = session.cancellation_generation;
        self.stop_owned(machine)?;
        self.event(machine, EventKind::Cancelled)
    }

    pub(super) fn stop_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine_allow_expired(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "stopped" {
            self.stop_owned(machine)?;
        }
        self.event(machine, EventKind::Stopped)
    }

    pub(super) fn collect_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Record, Error> {
        let mut machines = self.owned_machine_allow_expired(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        let exit_status = match machine.child.as_mut() {
            Some(child) => child.try_wait().map_err(|_| Error::Io)?,
            None => None,
        };
        if let Some(status) = exit_status {
            machine
                .supervision
                .store(2, std::sync::atomic::Ordering::Release);
            machine.child = None;
            machine.owner.state = "stopped".to_owned();
            machine.owner.pid = None;
            machine.owner.process_start_ticks = None;
            machine.owner.exit_code = status.code();
            CgroupV2::new(&self.cgroup_root)?.kill_and_remove(&handle.session_id)?;
            self.event(machine, EventKind::Collected)?;
        }
        Ok(Self::record(machine, session))
    }

    pub(super) fn release_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine_allow_expired(session, handle, now_unix_ms)?;
        let event = {
            let machine = machines
                .get_mut(&handle.session_id)
                .ok_or(Error::OwnershipMismatch)?;
            if machine.owner.state != "stopped" {
                self.stop_owned(machine)?;
            }
            machine.owner.state = "released".to_owned();
            let event = self.event(machine, EventKind::Released)?;
            machine
                .overlay
                .as_ref()
                .ok_or(Error::OwnershipMismatch)?
                .release_after_vm_stopped()
                .map_err(|_| Error::InvalidOverlay)?;
            fs::remove_dir_all(&machine.directory).map_err(|_| Error::Io)?;
            event
        };
        machines.remove(&handle.session_id);
        Ok(event)
    }
}
