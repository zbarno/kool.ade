use super::super::*;
use crate::computer_use::ComputerGuestChannelV1;
use std::{process::Command, thread, time::Instant};

impl LocalKvmComputerBackend {
    pub(super) fn boot_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "prepared" || machine.child.is_some() {
            return Err(Error::InvalidTransition);
        }
        let overlay = machine.overlay.as_ref().ok_or(Error::OwnershipMismatch)?;
        overlay
            .verify_before_boot()
            .map_err(|_| Error::InvalidOverlay)?;
        let control_socket = machine.directory.join("guest-control.sock");
        let display_socket = machine.directory.join("display.sock");
        if control_socket.exists() || display_socket.exists() {
            return Err(Error::OwnershipMismatch);
        }
        let args =
            commands::command_args(session, overlay.path(), &control_socket, &display_socket)
                .map_err(|_| Error::ResourceLimit)?;
        let qemu_path = self.qemu_path();
        process::validate_qemu_program(&qemu_path)?;
        let mut command = Command::new(qemu_path);
        commands::configure_command(&mut command, &args, &machine.directory);
        let cgroup = CgroupV2::new(&self.cgroup_root)?;
        process::set_child_limits(
            &mut command,
            session.policy.resource_budget.disk_bytes,
            &cgroup.process_path(&handle.session_id),
        )?;
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                cgroup.kill_and_remove(&handle.session_id)?;
                return Err(Error::QemuStartFailed);
            }
        };
        let pid = child.id();
        let start_ticks = match owned_process_start_ticks(pid, &machine.owner) {
            Ok(ticks) => ticks,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = cgroup.kill_and_remove(&handle.session_id);
                return Err(error);
            }
        };
        machine.owner.pid = Some(pid);
        machine.owner.process_start_ticks = Some(start_ticks);
        machine.owner.state = "booting".to_owned();
        if let Err(error) = with_registry_lock(&self.runtime_root, || {
            write_owner(&machine.directory, &machine.owner)
        }) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = cgroup.kill_and_remove(&handle.session_id);
            return Err(error);
        }
        machine.child = Some(child);
        machine
            .supervision
            .store(0, std::sync::atomic::Ordering::Release);
        let owner = machine.owner.clone();
        let directory = machine.directory.clone();
        let runtime_root = self.runtime_root.clone();
        let supervision = machine.supervision.clone();
        let cgroup_root = self.cgroup_root.clone();
        let stop_timeout = self.limits.stop_timeout;
        let readiness_timeout = self.limits.boot_timeout;
        let lifetime_ms = session
            .policy
            .resource_budget
            .time_limit_ms
            .min(session.lease.expires_at_unix_ms.saturating_sub(now_unix_ms));
        let lifetime = Duration::from_millis(lifetime_ms);
        std::thread::spawn(move || {
            super::super::watchdog::supervise(
                owner,
                directory,
                runtime_root,
                cgroup_root,
                stop_timeout,
                readiness_timeout,
                lifetime,
                supervision,
            );
        });
        self.event(machine, EventKind::Booted)
    }

    pub(super) fn wait_ready_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        timeout: Duration,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let bounded = timeout.min(self.limits.boot_timeout);
        let deadline = Instant::now() + bounded;
        while Instant::now() < deadline {
            {
                let mut machines = self.owned_machine(session, handle, now_unix_ms)?;
                let machine = machines
                    .get_mut(&handle.session_id)
                    .ok_or(Error::OwnershipMismatch)?;
                let supervision = machine
                    .supervision
                    .load(std::sync::atomic::Ordering::Acquire);
                if machine.owner.state == "connected" && supervision == 1 {
                    return Ok(Event {
                        sequence: machine
                            .owner
                            .guest_ready_sequence
                            .ok_or(Error::GuestReadinessUnverified)?,
                        session_id: machine.owner.session_id.clone(),
                        lease_generation: machine.owner.lease_generation,
                        kind: EventKind::GuestReady,
                    });
                }
                if supervision >= 3 {
                    return Err(Error::QemuTimedOut);
                }
                if machine.owner.state != "booting" {
                    return Err(Error::InvalidTransition);
                }
                if !process::is_process_alive(machine)? {
                    self.stop_owned(machine)?;
                    return Err(Error::QemuExited);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        let mut machines = self.owned_machine_allow_expired(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state == "connected"
            && machine
                .supervision
                .load(std::sync::atomic::Ordering::Acquire)
                == 1
        {
            return Ok(Event {
                sequence: machine
                    .owner
                    .guest_ready_sequence
                    .ok_or(Error::GuestReadinessUnverified)?,
                session_id: machine.owner.session_id.clone(),
                lease_generation: machine.owner.lease_generation,
                kind: EventKind::GuestReady,
            });
        }
        if machine
            .supervision
            .load(std::sync::atomic::Ordering::Acquire)
            >= 3
        {
            return Err(Error::QemuTimedOut);
        }
        if machine.owner.state != "booting" {
            return Err(Error::InvalidTransition);
        }
        self.stop_owned(machine)?;
        machine.owner.state = "timed_out".to_owned();
        self.event(machine, EventKind::TimedOut)?;
        Err(Error::QemuTimedOut)
    }

    pub(super) fn connect_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<ComputerGuestChannelV1, Error> {
        let machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "booting" && machine.owner.state != "connected" {
            return Err(Error::InvalidTransition);
        }
        let supervision = machine
            .supervision
            .load(std::sync::atomic::Ordering::Acquire);
        if supervision >= 3 || (machine.owner.state == "connected" && supervision != 1) {
            return Err(Error::QemuTimedOut);
        }
        #[cfg(unix)]
        {
            let socket = machine.directory.join("guest-control.sock");
            let stream = std::os::unix::net::UnixStream::connect(socket)
                .map_err(|_| Error::GuestChannelUnavailable)?;
            stream
                .set_read_timeout(Some(self.limits.boot_timeout))
                .map_err(|_| Error::Io)?;
            stream
                .set_write_timeout(Some(self.limits.boot_timeout))
                .map_err(|_| Error::Io)?;
            Ok(ComputerGuestChannelV1::new(
                handle.session_id.clone(),
                handle.lease_generation,
                stream,
            ))
        }
        #[cfg(not(unix))]
        Err(Error::UnsupportedHost)
    }
}
