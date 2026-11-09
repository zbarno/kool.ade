//! Fixed-argument Linux QEMU/KVM implementation.

mod commands;
mod display;
mod lifecycle;
mod policy;
mod process;
mod watchdog;

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::Child,
    sync::{Arc, Mutex, atomic::AtomicU8},
    time::Duration,
};

use ed25519_dalek::VerifyingKey;
use sha2::Digest;
use uuid::Uuid;

use crate::computer_use::network::InternetResearchGatewayV1;
use crate::computer_use::{
    ComputerBackendErrorV1 as Error, ComputerBackendEventKindV1 as EventKind,
    ComputerBackendEventV1 as Event, ComputerMachineHandleV1 as Handle,
    ComputerMachineRecordV1 as Record, ComputerMachineStateV1 as State,
    ComputerResourceLimitsV1 as Limits, ComputerSessionV1, guest_image::GuestImageOverlayV1,
};

use super::{
    registry::{
        MachineOwnerRecordV1, count_machine_slots, set_private_dir, with_registry_lock, write_owner,
    },
    types::validate_runtime_root,
};

use self::policy::{policy_fingerprint, target_machine_id, verify_record};
use self::process::{CgroupV2, owned_process_start_ticks, qemu_program, validate_qemu_program};

#[cfg(test)]
#[path = "qemu/tests.rs"]
mod tests;

fn boot_fingerprint(machine: &LiveMachine) -> Option<String> {
    let boot_id = machine.owner.guest_boot_id.as_ref()?;
    let transcript = machine.owner.guest_transcript_sha256.as_ref()?;
    let mut digest = sha2::Sha256::new();
    digest.update(b"koolade.computer-use-boot-fingerprint.v1\0");
    digest.update(boot_id.as_bytes());
    digest.update(transcript.as_bytes());
    Some(format!("sha256:{:x}", digest.finalize()))
}

struct LiveMachine {
    directory: PathBuf,
    overlay: Option<GuestImageOverlayV1>,
    owner: MachineOwnerRecordV1,
    child: Option<Child>,
    network_gateway: Option<InternetResearchGatewayV1>,
    supervision: Arc<AtomicU8>,
}

/// Operates only disposable, signed-image sessions and one unprivileged local
/// QEMU process per active slot. All runtime paths are rooted in app-owned data.
pub struct LocalKvmComputerBackend {
    runtime_root: PathBuf,
    overlay_root: PathBuf,
    cgroup_root: PathBuf,
    controller_key_id: String,
    controller_key: VerifyingKey,
    host_machine_id: String,
    limits: Limits,
    live: Mutex<HashMap<String, LiveMachine>>,
    #[cfg(test)]
    qemu_path_override: Option<PathBuf>,
}

impl LocalKvmComputerBackend {
    pub fn new(
        runtime_root: PathBuf,
        overlay_root: PathBuf,
        delegated_cgroup_root: PathBuf,
        host_machine_id: String,
        controller_key_id: String,
        controller_key: VerifyingKey,
        limits: Limits,
    ) -> Result<Self, Error> {
        if !Uuid::parse_str(&controller_key_id).is_ok_and(|id| id.to_string() == controller_key_id)
            || !Uuid::parse_str(&host_machine_id).is_ok_and(|id| id.to_string() == host_machine_id)
            || limits.max_vcpus == 0
            || limits.max_memory_bytes == 0
            || limits.max_overlay_bytes == 0
            || limits.max_processes == 0
            || limits.parallel_slots_per_machine == 0
            || limits.parallel_slots_per_machine > 16
            || limits.boot_timeout.is_zero()
            || limits.stop_timeout.is_zero()
        {
            return Err(Error::ResourceLimit);
        }
        Ok(Self {
            runtime_root,
            overlay_root,
            cgroup_root: delegated_cgroup_root,
            controller_key_id,
            controller_key,
            host_machine_id,
            limits,
            live: Mutex::new(HashMap::new()),
            #[cfg(test)]
            qemu_path_override: None,
        })
    }

    fn qemu_path(&self) -> PathBuf {
        #[cfg(test)]
        if let Some(path) = &self.qemu_path_override {
            return path.clone();
        }
        qemu_program()
    }

    fn validate_session(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<(), Error> {
        session
            .validate_for_host(now_unix_ms, &self.controller_key_id, &self.controller_key)
            .map_err(|_| Error::UntrustedSession)?;
        if session.identity.session_id != handle.session_id
            || session.lease.lease_id != handle.lease_id
            || session.lease.generation != handle.lease_generation
            || session.lease.owner_id != session.identity.user_id
            || target_machine_id(session) != handle.machine_id
            || handle.machine_id != self.host_machine_id
        {
            return Err(Error::StaleLease);
        }
        Ok(())
    }

    fn validate_signed_session_allow_expired(
        &self,
        session: &ComputerSessionV1,
        now_unix_ms: u64,
    ) -> Result<(), Error> {
        if session.lease.issued_at_unix_ms > now_unix_ms {
            return Err(Error::StaleLease);
        }
        session
            .validate_for_host(
                session.lease.issued_at_unix_ms,
                &self.controller_key_id,
                &self.controller_key,
            )
            .map_err(|_| Error::UntrustedSession)
    }

    fn owned_machine<'a>(
        &'a self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<std::sync::MutexGuard<'a, HashMap<String, LiveMachine>>, Error> {
        self.validate_session(session, handle, now_unix_ms)?;
        let machines = self.live.lock().map_err(|_| Error::Io)?;
        let machine = machines
            .get(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        verify_record(&machine.owner, session, handle)?;
        Ok(machines)
    }

    fn owned_machine_allow_expired<'a>(
        &'a self,
        session: &ComputerSessionV1,
        handle: &Handle,
        now_unix_ms: u64,
    ) -> Result<std::sync::MutexGuard<'a, HashMap<String, LiveMachine>>, Error> {
        self.validate_signed_session_allow_expired(session, now_unix_ms)?;
        if session.identity.session_id != handle.session_id
            || session.lease.lease_id != handle.lease_id
            || session.lease.generation < handle.lease_generation
            || target_machine_id(session) != self.host_machine_id
            || handle.machine_id != self.host_machine_id
        {
            return Err(Error::StaleLease);
        }
        let machines = self.live.lock().map_err(|_| Error::Io)?;
        let machine = machines
            .get(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        verify_record(&machine.owner, session, handle)?;
        Ok(machines)
    }

    fn event(&self, machine: &mut LiveMachine, kind: EventKind) -> Result<Event, Error> {
        with_registry_lock(&self.runtime_root, || {
            let persisted = super::registry::read_owner(&machine.directory)?;
            if persisted.state == "timed_out"
                && !matches!(machine.owner.state.as_str(), "timed_out" | "stopped")
            {
                return Err(Error::InvalidTransition);
            }
            machine.owner.event_sequence = machine
                .owner
                .event_sequence
                .max(persisted.event_sequence)
                .checked_add(1)
                .ok_or(Error::ResourceLimit)?;
            if kind == EventKind::GuestReady {
                machine.owner.guest_ready_sequence = Some(machine.owner.event_sequence);
            }
            write_owner(&machine.directory, &machine.owner)?;
            Ok(())
        })?;
        Ok(Event {
            sequence: machine.owner.event_sequence,
            session_id: machine.owner.session_id.clone(),
            lease_generation: machine.owner.lease_generation,
            kind,
            boot_fingerprint: boot_fingerprint(machine),
        })
    }

    fn record(machine: &LiveMachine, session: &ComputerSessionV1) -> Record {
        Record {
            session_id: machine.owner.session_id.clone(),
            machine_id: machine.owner.machine_id.clone(),
            lease_id: machine.owner.lease_id.clone(),
            lease_generation: machine.owner.lease_generation,
            image_digest: machine.owner.image_digest.clone(),
            policy_fingerprint: machine.owner.policy_fingerprint.clone(),
            state: if machine
                .supervision
                .load(std::sync::atomic::Ordering::Acquire)
                >= 3
            {
                State::TimedOut
            } else {
                match machine.owner.state.as_str() {
                    "prepared" => State::Prepared,
                    "booting" => State::Booting,
                    "connected" => State::Connected,
                    "suspended" => State::Suspended,
                    "timed_out" => State::TimedOut,
                    _ => State::Stopped,
                }
            },
            memory_bytes: session.policy.resource_budget.memory_bytes,
            disk_bytes: session.policy.resource_budget.disk_bytes,
            vcpus: session.policy.resource_budget.cpu_millis.div_ceil(1_000) as u16,
            overlay_bytes: fs::metadata(&machine.owner.overlay_path)
                .ok()
                .map(|metadata| metadata.len()),
            exit_code: machine.owner.exit_code,
        }
    }

    fn stop_owned(&self, machine: &mut LiveMachine) -> Result<(), Error> {
        if let Some(gateway) = machine.network_gateway.as_mut() {
            gateway.stop();
        }
        if let Some(child) = machine.child.as_mut() {
            process::terminate_owned(child, &machine.owner, &self.limits.stop_timeout)?;
            machine.child = None;
        } else if let Some(pid) = machine.owner.pid {
            process::terminate_recovered(pid, &machine.owner, &self.limits.stop_timeout)?;
        }
        CgroupV2::new(&self.cgroup_root)?.kill_and_remove(&machine.owner.session_id)?;
        machine.owner.state = "stopped".to_owned();
        machine.owner.pid = None;
        machine.owner.process_start_ticks = None;
        with_registry_lock(&self.runtime_root, || {
            write_owner(&machine.directory, &machine.owner)
        })?;
        machine
            .supervision
            .store(2, std::sync::atomic::Ordering::Release);
        Ok(())
    }
}

pub(super) fn verify_qemu_launcher_containment() -> Result<(), Error> {
    validate_qemu_program(&qemu_program())
}
