//! Worker-host slot leases shared by every authenticated SSH controller.

use fs2::FileExt;
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) const MAX_REMOTE_EXECUTIONS: u32 = 8;

pub(crate) struct WorkerCapacityPool {
    state_root: PathBuf,
    directory: PathBuf,
    worker_id: String,
    max_jobs: u32,
}

pub(crate) struct WorkerCapacityLease {
    slot: File,
    admission_lock_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ResourceReservation {
    pub(crate) cpu_millis: u32,
    pub(crate) memory_bytes: u64,
}

struct SlotReservation {
    execution_id: String,
    resources: ResourceReservation,
}

impl WorkerCapacityPool {
    pub(crate) fn new_local(root: &std::path::Path) -> anyhow::Result<Self> {
        Self::new(root, "local".into())
    }

    pub(crate) fn new_local_unknown(root: &std::path::Path) -> anyhow::Result<Self> {
        Self::new_local(root)
    }

    pub(crate) fn new(root: &std::path::Path, worker_id: String) -> anyhow::Result<Self> {
        if worker_id != "local" {
            crate::execution::contract::validate_identifier(&worker_id)?;
        }
        let directory = root.join("worker-capacity").join(&worker_id);
        fs::create_dir_all(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "worker capacity directory is not trusted"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            state_root: root.to_path_buf(),
            directory,
            worker_id,
            max_jobs: MAX_REMOTE_EXECUTIONS,
        })
    }

    pub(crate) fn max_jobs(&self) -> u32 {
        self.max_jobs
    }

    #[cfg(test)]
    pub(crate) fn acquire(&self) -> anyhow::Result<WorkerCapacityLease> {
        self.acquire_inner(None)
    }

    pub(crate) fn acquire_resources(
        &self,
        execution_id: &str,
        requested_cpu_millis: u32,
        requested_memory_bytes: u64,
        capacity_cpu_millis: u32,
        capacity_memory_bytes: u64,
    ) -> anyhow::Result<WorkerCapacityLease> {
        anyhow::ensure!(
            capacity_cpu_millis > 0,
            "worker CPU capacity is unavailable"
        );
        anyhow::ensure!(
            capacity_memory_bytes > 0,
            "worker memory capacity is unavailable"
        );
        self.acquire_inner(Some((
            execution_id.to_owned(),
            requested_cpu_millis,
            requested_memory_bytes,
            capacity_cpu_millis,
            capacity_memory_bytes,
        )))
    }

    pub(crate) fn acquire_unknown_local(&self) -> anyhow::Result<WorkerCapacityLease> {
        anyhow::ensure!(
            self.worker_id == "local",
            "unknown capacity lease is local-only"
        );
        self.acquire_inner(None)
    }

    pub(crate) fn reserved_resources(&self) -> anyhow::Result<ResourceReservation> {
        let admission = open_lock_file(&self.admission_lock_path())?;
        FileExt::lock_exclusive(&admission)?;
        let result = self.scan_slots().and_then(|(_, mut total, active)| {
            self.add_durable_reservations(&mut total, &active)?;
            Ok(total)
        });
        let _ = FileExt::unlock(&admission);
        result
    }

    fn acquire_inner(
        &self,
        resources: Option<(String, u32, u64, u32, u64)>,
    ) -> anyhow::Result<WorkerCapacityLease> {
        let admission = open_lock_file(&self.admission_lock_path())?;
        FileExt::lock_exclusive(&admission)?;
        let result = self.acquire_locked(resources);
        let _ = FileExt::unlock(&admission);
        result
    }

    fn acquire_locked(
        &self,
        resources: Option<(String, u32, u64, u32, u64)>,
    ) -> anyhow::Result<WorkerCapacityLease> {
        let (candidate, mut reserved, active) = self.scan_slots()?;
        let durable = self.add_durable_reservations(&mut reserved, &active)?;
        anyhow::ensure!(
            self.worker_id != "local"
                || resources.is_some()
                || (active.is_empty() && durable.is_empty()),
            "unfinished local execution still owns host capacity"
        );
        anyhow::ensure!(
            self.worker_id != "local" || resources.is_none() || !active.contains("-"),
            "unknown-capacity local execution owns host capacity"
        );
        let slot =
            candidate.ok_or_else(|| anyhow::anyhow!("remote worker execution capacity is full"))?;
        if let Some((
            execution_id,
            requested_cpu,
            requested_memory,
            capacity_cpu,
            capacity_memory,
        )) = resources.as_ref()
        {
            anyhow::ensure!(
                !active.contains(execution_id) && !durable.contains(execution_id),
                "worker execution already has a capacity reservation"
            );
            let total_cpu = reserved
                .cpu_millis
                .checked_add(*requested_cpu)
                .ok_or_else(|| anyhow::anyhow!("worker CPU reservation overflow"))?;
            anyhow::ensure!(total_cpu <= *capacity_cpu, "worker CPU capacity is full");
            let total_memory = reserved
                .memory_bytes
                .checked_add(*requested_memory)
                .ok_or_else(|| anyhow::anyhow!("worker memory reservation overflow"))?;
            anyhow::ensure!(
                total_memory <= *capacity_memory,
                "worker memory capacity is full"
            );
        }
        let reservation = resources.as_ref().map_or(
            ResourceReservation::default(),
            |(_, cpu_millis, memory_bytes, _, _)| ResourceReservation {
                cpu_millis: *cpu_millis,
                memory_bytes: *memory_bytes,
            },
        );
        let execution_id = resources.as_ref().map_or_else(
            || "-".to_owned(),
            |(execution_id, _, _, _, _)| execution_id.clone(),
        );
        write_reservation(&slot, &execution_id, reservation)?;
        Ok(WorkerCapacityLease {
            slot,
            admission_lock_path: self.admission_lock_path(),
        })
    }

    fn scan_slots(&self) -> anyhow::Result<(Option<File>, ResourceReservation, HashSet<String>)> {
        let mut candidate = None;
        let mut reserved = ResourceReservation::default();
        let mut active_ids = HashSet::new();
        for slot in 0..self.max_jobs {
            let file = open_lock_file(&self.slot_path(slot))?;
            match FileExt::try_lock_exclusive(&file) {
                Ok(()) if candidate.is_none() => {
                    write_reservation(&file, "-", ResourceReservation::default())?;
                    candidate = Some(file);
                }
                Ok(()) => {
                    write_reservation(&file, "-", ResourceReservation::default())?;
                    FileExt::unlock(&file)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let active = read_reservation(&file)?;
                    active_ids.insert(active.execution_id.clone());
                    reserved.cpu_millis = reserved
                        .cpu_millis
                        .checked_add(active.resources.cpu_millis)
                        .ok_or_else(|| anyhow::anyhow!("worker CPU reservation overflow"))?;
                    reserved.memory_bytes = reserved
                        .memory_bytes
                        .checked_add(active.resources.memory_bytes)
                        .ok_or_else(|| anyhow::anyhow!("worker memory reservation overflow"))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok((candidate, reserved, active_ids))
    }

    fn slot_path(&self, slot: u32) -> PathBuf {
        self.directory.join(format!("slot-{slot}.lock"))
    }

    fn admission_lock_path(&self) -> PathBuf {
        self.directory.join("admission.lock")
    }
}

impl Drop for WorkerCapacityLease {
    fn drop(&mut self) {
        if let Ok(admission) = open_lock_file(&self.admission_lock_path)
            && FileExt::lock_exclusive(&admission).is_ok()
        {
            let _ = write_reservation(&self.slot, "-", ResourceReservation::default());
            let _ = FileExt::unlock(&admission);
        }
        let _ = FileExt::unlock(&self.slot);
    }
}

fn open_lock_file(path: &Path) -> anyhow::Result<File> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("worker capacity lock is not trusted");
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}

fn read_reservation(file: &File) -> anyhow::Result<SlotReservation> {
    let mut contents = String::new();
    file.try_clone()?.read_to_string(&mut contents)?;
    let mut values = contents.split_ascii_whitespace();
    let execution_id = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("worker reservation metadata is invalid"))?
        .to_owned();
    let cpu_millis = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("worker reservation metadata is invalid"))?
        .parse::<u32>()?;
    let memory_bytes = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("worker reservation metadata is invalid"))?
        .parse::<u64>()?;
    anyhow::ensure!(
        values.next().is_none(),
        "worker reservation metadata is invalid"
    );
    Ok(SlotReservation {
        execution_id,
        resources: ResourceReservation {
            cpu_millis,
            memory_bytes,
        },
    })
}

fn write_reservation(
    file: &File,
    execution_id: &str,
    reservation: ResourceReservation,
) -> anyhow::Result<()> {
    let mut file = file.try_clone()?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(
        file,
        "{} {} {}",
        execution_id, reservation.cpu_millis, reservation.memory_bytes
    )?;
    file.sync_data()?;
    Ok(())
}

mod durable;
#[cfg(test)]
#[path = "worker_capacity/tests.rs"]
mod tests;
