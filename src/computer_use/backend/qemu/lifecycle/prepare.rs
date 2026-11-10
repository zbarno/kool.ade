use super::super::super::registry::read_owner;
use super::super::*;
use crate::computer_use::{ComputerBackendProbeV1 as Probe, ComputerSessionClassV1};
use std::fs::{self, OpenOptions};
use std::sync::{Arc, atomic::AtomicU8};

mod overlay_guard;
use overlay_guard::OverlayPreparationGuard;

impl LocalKvmComputerBackend {
    pub(super) fn probe_impl(&self) -> Result<Probe, Error> {
        if unsafe { libc::geteuid() } == 0 {
            return Err(Error::UnsupportedHost);
        }
        crate::computer_use::process_security::verify_child_restrictions()
            .map_err(|_| Error::UnsupportedHost)?;
        let kvm_accessible = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .is_ok();
        if !kvm_accessible {
            return Err(Error::HostNotReady);
        }
        let qemu_path = self.qemu_path();
        validate_qemu_program(&qemu_path)?;
        let cgroups = CgroupV2::new(&self.cgroup_root)?;
        cgroups.probe()?;
        Ok(Probe {
            linux_host: true,
            kvm_accessible,
            qemu_available: true,
            delegated_resource_controls: true,
            max_vcpus: self.limits.max_vcpus,
            max_memory_bytes: self.limits.max_memory_bytes,
            max_overlay_bytes: self.limits.max_overlay_bytes,
            max_processes: self.limits.max_processes,
            parallel_slots_per_machine: self.limits.parallel_slots_per_machine,
            boot_timeout: self.limits.boot_timeout,
        })
    }

    pub(super) fn prepare_impl(
        &self,
        session: &ComputerSessionV1,
        overlay: GuestImageOverlayV1,
        now_unix_ms: u64,
    ) -> Result<(Handle, Event), Error> {
        let mut overlay = OverlayPreparationGuard(Some(overlay));
        session
            .validate_for_host(now_unix_ms, &self.controller_key_id, &self.controller_key)
            .map_err(|_| Error::UntrustedSession)?;
        let persistent_personal =
            session.session_class == ComputerSessionClassV1::PersonalPersistent;
        let expected_overlay_root = if persistent_personal {
            self.personal_overlay_root()
        } else {
            self.overlay_root.clone()
        };
        if !matches!(
            session.session_class,
            ComputerSessionClassV1::Disposable | ComputerSessionClassV1::PersonalPersistent
        ) || persistent_personal != overlay.is_persistent_personal()
            || !matches!(
                session.network_profile,
                crate::computer_use::ComputerNetworkProfileV1::Disabled
                    | crate::computer_use::ComputerNetworkProfileV1::InternetResearch
            )
            || session.identity.session_id != overlay.session_id()
            || session.image_digest != overlay.image_digest()
            || session.image_manifest_digest != overlay.manifest_digest()
            || session.image_signer_key_id != overlay.signer_key_id()
            || target_machine_id(session) != self.host_machine_id
            || overlay.directory() != expected_overlay_root
            || overlay.path().parent() != Some(expected_overlay_root.as_path())
            || (!persistent_personal
                && !overlay_filename_matches_session(overlay.path(), &session.identity.session_id))
            || (persistent_personal && !overlay_filename_matches_profile(overlay.path()))
        {
            return Err(Error::UnsupportedPolicy);
        }
        let overlay_metadata =
            fs::symlink_metadata(overlay.path()).map_err(|_| Error::InvalidOverlay)?;
        if !overlay_metadata.file_type().is_file() {
            return Err(Error::InvalidOverlay);
        }
        self.probe_impl()?;
        self.check_resource_budget(session, &overlay)?;
        validate_runtime_root(&self.runtime_root)?;
        validate_runtime_root(&expected_overlay_root)?;

        let session_id = session.identity.session_id.clone();
        let machine_id = target_machine_id(session).to_owned();
        let directory = self.runtime_root.join("sessions").join(&session_id);
        let result = with_registry_lock(&self.runtime_root, || {
            if count_machine_slots(&self.runtime_root, &machine_id)?
                >= self.limits.parallel_slots_per_machine as usize
            {
                return Err(Error::MachineSlotLimit);
            }
            fs::create_dir_all(directory.parent().ok_or(Error::UnsafeRuntimeDirectory)?)
                .map_err(|_| Error::Io)?;
            set_private_dir(directory.parent().ok_or(Error::UnsafeRuntimeDirectory)?)?;
            fs::create_dir(&directory).map_err(|_| Error::DuplicateSession)?;
            set_private_dir(&directory)?;
            CgroupV2::new(&self.cgroup_root)?.create(session, self.limits.max_processes)?;
            let policy_fingerprint = policy_fingerprint(session)?;
            let owner = MachineOwnerRecordV1 {
                schema_version: 1,
                owner_id: session.identity.user_id.clone(),
                project_id: session.identity.project_id.clone(),
                task_id: session.identity.task_id.clone(),
                session_id: session_id.clone(),
                machine_id: machine_id.clone(),
                lease_id: session.lease.lease_id.clone(),
                lease_generation: session.lease.generation,
                cancellation_generation: session.cancellation_generation,
                policy_fingerprint,
                image_digest: overlay.image_digest().to_owned(),
                image_manifest_digest: overlay.manifest_digest().to_owned(),
                image_signer_key_id: overlay.signer_key_id().to_owned(),
                catalog_generation: overlay.catalog_generation(),
                overlay_path: overlay.path().to_path_buf(),
                persistent_personal,
                state: "prepared".to_owned(),
                event_sequence: 0,
                guest_boot_id: None,
                guest_transcript_sha256: None,
                guest_ready_sequence: None,
                pid: None,
                process_start_ticks: None,
                exit_code: None,
            };
            write_owner(&directory, &owner)?;
            Ok(owner)
        });
        let owner = match result {
            Ok(owner) => owner,
            Err(error) => {
                let _ = CgroupV2::new(&self.cgroup_root)
                    .and_then(|cgroup| cgroup.kill_and_remove(&session_id));
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        let handle = Handle {
            session_id: session_id.clone(),
            lease_id: session.lease.lease_id.clone(),
            lease_generation: session.lease.generation,
            machine_id,
        };
        let mut machine = LiveMachine {
            directory: directory.clone(),
            overlay: overlay.0.take(),
            owner,
            child: None,
            network_gateway: None,
            supervision: Arc::new(AtomicU8::new(2)),
        };
        let event = match self.event(&mut machine, EventKind::Prepared) {
            Ok(event) => event,
            Err(error) => {
                self.cleanup_unregistered_machine(&mut machine, &session_id);
                return Err(error);
            }
        };
        let mut live = match self.live.lock() {
            Ok(live) => live,
            Err(_) => {
                self.cleanup_unregistered_machine(&mut machine, &session_id);
                return Err(Error::Io);
            }
        };
        live.insert(session_id, machine);
        Ok((handle, event))
    }

    fn cleanup_unregistered_machine(&self, machine: &mut LiveMachine, session_id: &str) {
        CgroupV2::new(&self.cgroup_root)
            .and_then(|cgroup| cgroup.kill_and_remove(session_id))
            .ok();
        if let Some(overlay) = machine.overlay.take() {
            overlay.release_after_vm_stopped().ok();
        }
        fs::remove_dir_all(&machine.directory).ok();
    }

    pub(super) fn recover_impl(&self) -> Result<Vec<Event>, Error> {
        validate_runtime_root(&self.runtime_root)?;
        validate_runtime_root(&self.overlay_root)?;
        validate_runtime_root(&self.personal_overlay_root())?;
        if !self.live.lock().map_err(|_| Error::Io)?.is_empty() {
            return Err(Error::InvalidTransition);
        }
        let sessions = self.runtime_root.join("sessions");
        if !sessions.exists() {
            return Ok(Vec::new());
        }
        let cgroups = CgroupV2::new(&self.cgroup_root)?;
        with_registry_lock(&self.runtime_root, || {
            let mut recovered = Vec::new();
            for entry in fs::read_dir(&sessions).map_err(|_| Error::Io)? {
                let entry = entry.map_err(|_| Error::Io)?;
                let directory = entry.path();
                let metadata =
                    fs::symlink_metadata(&directory).map_err(|_| Error::OwnershipMismatch)?;
                if !metadata.file_type().is_dir() {
                    return Err(Error::OwnershipMismatch);
                }
                let owner = read_owner(&directory)?;
                validate_recovery_owner(
                    &owner,
                    &directory,
                    &self.overlay_root,
                    &self.host_machine_id,
                )?;
                let mut process_owner = owner.clone();
                if let Some((pid, start_ticks)) = process::find_orphaned_qemu(&owner)? {
                    process_owner.pid = Some(pid);
                    process_owner.process_start_ticks = Some(start_ticks);
                    process::terminate_recovered(pid, &process_owner, &self.limits.stop_timeout)?;
                }
                cgroups.kill_and_remove(&owner.session_id)?;
                if !owner.persistent_personal {
                    remove_owned_overlay(&owner.overlay_path)?;
                }
                recovered.push(Event {
                    sequence: owner
                        .event_sequence
                        .checked_add(1)
                        .ok_or(Error::ResourceLimit)?,
                    session_id: owner.session_id.clone(),
                    lease_generation: owner.lease_generation,
                    kind: EventKind::Recovered,
                    boot_fingerprint: None,
                });
                fs::remove_dir_all(&directory).map_err(|_| Error::Io)?;
            }
            Ok(recovered)
        })
    }
}

fn validate_recovery_owner(
    owner: &MachineOwnerRecordV1,
    directory: &std::path::Path,
    overlay_root: &std::path::Path,
    host_machine_id: &str,
) -> Result<(), Error> {
    let session_id = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::OwnershipMismatch)?;
    let expected_overlay_root = if owner.persistent_personal {
        overlay_root.join("personal")
    } else {
        overlay_root.to_path_buf()
    };
    if owner.schema_version != 1
        || owner.session_id != session_id
        || owner.machine_id != host_machine_id
        || !is_canonical_uuid(&owner.owner_id)
        || !is_canonical_uuid(&owner.project_id)
        || !is_canonical_uuid(&owner.task_id)
        || !is_canonical_uuid(&owner.lease_id)
        || owner.lease_generation == 0
        || owner.cancellation_generation == 0
        || owner.overlay_path.parent() != Some(expected_overlay_root.as_path())
        || (!owner.persistent_personal
            && !overlay_filename_matches_session(&owner.overlay_path, session_id))
        || (owner.persistent_personal && !overlay_filename_matches_profile(&owner.overlay_path))
        || owner.overlay_path.extension().and_then(|ext| ext.to_str()) != Some("qcow2")
        || !matches!(
            owner.state.as_str(),
            "prepared"
                | "booting"
                | "connected"
                | "suspended"
                | "timed_out"
                | "stopped"
                | "released"
        )
    {
        return Err(Error::OwnershipMismatch);
    }
    Ok(())
}

fn overlay_filename_matches_session(path: &std::path::Path, session_id: &str) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    stem.strip_prefix(&format!("{session_id}-"))
        .is_some_and(is_canonical_uuid)
}

fn overlay_filename_matches_profile(path: &std::path::Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    uuid::Uuid::parse_str(stem).is_ok_and(|profile| profile.to_string() == stem)
}

fn remove_owned_overlay(path: &std::path::Path) -> Result<(), Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Error::Io),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(Error::OwnershipMismatch);
        }
    }
    if !metadata.file_type().is_file() {
        return Err(Error::OwnershipMismatch);
    }
    fs::remove_file(path).map_err(|_| Error::Io)
}

fn is_canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

#[cfg(test)]
#[path = "prepare/tests.rs"]
mod tests;
