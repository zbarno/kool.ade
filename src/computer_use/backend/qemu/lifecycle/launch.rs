use super::super::*;
use crate::computer_use::ComputerGuestChannelV1;
use std::{process::Command, thread, time::Instant};

#[path = "launch/boot_config.rs"]
mod boot_config;

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
        let secret_file = overlay
            .persistent_key_fd()
            .map_err(|_| Error::InvalidOverlay)?;
        let secret_fd = secret_file.as_ref().map(std::os::fd::AsRawFd::as_raw_fd);
        let control_socket = machine.directory.join("guest-control.sock");
        let display_socket = machine.directory.join("display.sock");
        let boot_config = machine.directory.join("guest-boot-config.json");
        let app_helper_config = machine.directory.join("guest-app-helper-config.json");
        let application_catalog = self
            .application_catalog_bytes
            .as_ref()
            .map(|_| machine.directory.join("guest-app-catalog.json"));
        if control_socket.exists() || display_socket.exists() {
            return Err(Error::OwnershipMismatch);
        }
        boot_config::write_boot_config(session, &boot_config)?;
        let mut network_gateway = if session.network_profile
            == crate::computer_use::ComputerNetworkProfileV1::InternetResearch
        {
            let lifetime_ms = session
                .policy
                .resource_budget
                .time_limit_ms
                .min(session.lease.expires_at_unix_ms.saturating_sub(now_unix_ms));
            let lifetime = Duration::from_millis(lifetime_ms);
            Some(
                crate::computer_use::network::InternetResearchGatewayV1::start(lifetime)
                    .map_err(|_| Error::UnsupportedPolicy)?,
            )
        } else {
            None
        };
        let research_proxy = network_gateway
            .as_ref()
            .map(crate::computer_use::network::InternetResearchGatewayV1::address);
        boot_config::write_app_helper_config(
            session,
            &self.controller_key_id,
            &self.controller_key,
            &app_helper_config,
        )?;
        if let (Some(bytes), Some(path)) = (
            self.application_catalog_bytes.as_deref(),
            application_catalog.as_deref(),
        ) {
            boot_config::write_application_catalog(bytes, path)?;
        }
        let args = commands::command_args(
            session,
            overlay.path(),
            &control_socket,
            &display_socket,
            &boot_config,
            &app_helper_config,
            application_catalog.as_deref(),
            research_proxy,
            secret_fd,
        )?;
        let qemu_path = self.qemu_path();
        process::validate_qemu_program(&qemu_path)?;
        let mut command = Command::new(qemu_path);
        commands::configure_command(&mut command, &args, &machine.directory);
        let cgroup = CgroupV2::new(&self.cgroup_root)?;
        process::set_child_limits(
            &mut command,
            session.policy.resource_budget.disk_bytes,
            &cgroup.process_path(&handle.session_id),
            secret_fd,
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
        machine.network_gateway = network_gateway.take();
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
                        boot_fingerprint: super::super::boot_fingerprint(machine),
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
                boot_fingerprint: super::super::boot_fingerprint(machine),
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

    pub fn authenticate_guest(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1, Error> {
        let (config, state, ready_boot_id) = {
            let machines = self.owned_machine(session, handle, now_unix_ms)?;
            let machine = machines
                .get(&handle.session_id)
                .ok_or(Error::OwnershipMismatch)?;
            let config =
                crate::computer_use::guest_protocol::GuestBootConfigV1::read_private_host_file(
                    &machine.directory.join("guest-boot-config.json"),
                )
                .map_err(|_| Error::GuestReadinessUnverified)?;
            (
                config,
                machine.owner.state.clone(),
                machine.owner.guest_boot_id.clone(),
            )
        };
        let channel = self.connect_impl(session, handle, now_unix_ms)?;
        let (authenticated, readiness) =
            crate::computer_use::guest_protocol::GuestHostTransportV1::authenticate(
                channel,
                config,
                session,
                now_unix_ms,
            )
            .map_err(|_| Error::GuestReadinessUnverified)?;
        match state.as_str() {
            "booting" => {
                self.mark_guest_ready_impl(session, handle, readiness, now_unix_ms)?;
            }
            "connected" if ready_boot_id.as_deref() == Some(readiness.guest_boot_id.as_str()) => {}
            _ => return Err(Error::GuestReadinessUnverified),
        }
        Ok(authenticated)
    }

    pub fn open_guest_action_journal(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<crate::computer_use::guest_protocol::GuestActionJournalV1, Error> {
        let machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "connected"
            || machine
                .supervision
                .load(std::sync::atomic::Ordering::Acquire)
                != 1
        {
            return Err(Error::GuestReadinessUnverified);
        }
        crate::computer_use::guest_protocol::GuestActionJournalV1::open(
            &machine.directory,
            session.identity.session_id.clone(),
            session.lease.generation,
            session.action_sequence,
            session.policy.resource_budget.action_limit,
        )
        .map_err(|_| Error::Io)
    }
}
