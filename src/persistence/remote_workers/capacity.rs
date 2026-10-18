//! Cross-process reservations for enrolled remote worker capacity.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

const STORE: &str = "remote-worker-capacity.json";
const LOCK: &str = "remote-worker-capacity.lock";
const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
const LEASE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

mod scheduler_cursor;
pub use scheduler_cursor::{
    order_auto_workers, record_auto_worker_selection, reserve_auto_capacity,
};

// POSIX record locks are process scoped, so two independent handles in this
// process can otherwise both enter the read/modify/write section. Keep a
// process-local guard in addition to the file lock used across processes.
static PROCESS_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Reservation {
    id: String,
    worker_id: String,
    host_key_fingerprint: String,
    expires_at_unix_ms: u64,
    #[serde(default = "default_pending_start")]
    pending_start: bool,
}

fn default_pending_start() -> bool {
    true
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Store {
    schema_version: u8,
    reservations: Vec<Reservation>,
    #[serde(default)]
    last_auto_worker_id: Option<String>,
}

/// A durable worker slot reservation. Dropping releases it; a crashed owner
/// is recovered after the bounded lease expiry.
pub struct RemoteWorkerCapacityLease {
    root: PathBuf,
    reservation: Reservation,
}

/// Fresh load and enrolled identity used to reserve an Auto route atomically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoWorkerCapacityCandidate {
    pub worker_id: String,
    pub host_key_fingerprint: String,
    pub max_jobs: u32,
    pub observed_running_jobs: u32,
}

impl std::fmt::Debug for RemoteWorkerCapacityLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteWorkerCapacityLease")
            .field("worker_id", &self.reservation.worker_id)
            .field("id", &self.reservation.id)
            .finish()
    }
}

impl RemoteWorkerCapacityLease {
    pub fn worker_id(&self) -> &str {
        &self.reservation.worker_id
    }

    pub fn matches_worker(&self, worker_id: &str, host_key_fingerprint: &str) -> bool {
        self.reservation.worker_id == worker_id
            && self.reservation.host_key_fingerprint == host_key_fingerprint
    }

    /// Recheck admission against a fresh worker load and current limit while
    /// holding the cross-process reservation lock.
    pub fn revalidate(&self, max_jobs: u32, observed_running_jobs: u32) -> anyhow::Result<()> {
        anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
        with_store(&self.root, |store| {
            let now = now_unix_ms();
            prune(store, now);
            anyhow::ensure!(
                store.reservations.iter().any(|item| {
                    item.id == self.reservation.id
                        && item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                }),
                "remote worker capacity reservation expired or changed"
            );
            let active = store
                .reservations
                .iter()
                .filter(|item| {
                    item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                        && item.pending_start
                })
                .count();
            anyhow::ensure!(
                observed_running_jobs.saturating_add(active as u32) <= max_jobs,
                "remote worker capacity changed before the session was established"
            );
            Ok(())
        })
    }

    /// Extends the crash-recovery window while this controller still owns the
    /// live execution session.
    pub fn renew(&mut self) -> anyhow::Result<()> {
        let now = now_unix_ms();
        with_store(&self.root, |store| {
            prune(store, now);
            let index = store
                .reservations
                .iter()
                .position(|item| item.id == self.reservation.id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            let item = &store.reservations[index];
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            let expires_at_unix_ms = now.saturating_add(ttl_ms());
            store.reservations[index].expires_at_unix_ms = expires_at_unix_ms;
            self.reservation.expires_at_unix_ms = expires_at_unix_ms;
            Ok(())
        })
    }

    /// Atomically reacquire a pending start slot against fresh worker load.
    pub fn prepare_start(
        &mut self,
        max_jobs: u32,
        observed_running_jobs: u32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
        let now = now_unix_ms();
        with_store(&self.root, |store| {
            prune(store, now);
            let index = store
                .reservations
                .iter()
                .position(|item| item.id == self.reservation.id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            let item = &store.reservations[index];
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            let was_pending = item.pending_start;
            let pending = store
                .reservations
                .iter()
                .filter(|item| {
                    item.worker_id == self.reservation.worker_id
                        && item.host_key_fingerprint == self.reservation.host_key_fingerprint
                        && item.pending_start
                })
                .count();
            let pending = pending + usize::from(!was_pending);
            anyhow::ensure!(
                observed_running_jobs.saturating_add(pending as u32) <= max_jobs,
                "remote worker has no unreserved execution slots"
            );
            let expires_at_unix_ms = now.saturating_add(ttl_ms());
            let item = &mut store.reservations[index];
            item.expires_at_unix_ms = expires_at_unix_ms;
            item.pending_start = true;
            self.reservation.expires_at_unix_ms = expires_at_unix_ms;
            self.reservation.pending_start = true;
            Ok(())
        })
    }

    /// Mark the reserved execution as running after the worker accepts start.
    /// Fresh worker probes account for this lease from that point onward.
    pub fn mark_running(&mut self) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            let now = now_unix_ms();
            prune(store, now);
            let item = store
                .reservations
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            item.pending_start = false;
            self.reservation.pending_start = false;
            Ok(())
        })
    }

    /// Undo a start reservation only when the worker explicitly rejected the
    /// start before accepting it. Ambiguous transport failures must keep the
    /// pending reservation until its lease expires.
    pub fn rollback_rejected_start(&mut self) -> anyhow::Result<()> {
        self.rollback_rejected_start_at(now_unix_ms())
    }

    fn rollback_rejected_start_at(&mut self, now: u64) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            prune(store, now);
            let item = store
                .reservations
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("remote worker capacity lease expired"))?;
            anyhow::ensure!(
                item.worker_id == self.reservation.worker_id
                    && item.host_key_fingerprint == self.reservation.host_key_fingerprint,
                "remote worker capacity lease identity changed"
            );
            item.pending_start = false;
            self.reservation.pending_start = false;
            Ok(())
        })
    }

    pub fn release(mut self) -> anyhow::Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> anyhow::Result<()> {
        let id = self.reservation.id.clone();
        with_store(&self.root, |store| {
            store.reservations.retain(|item| item.id != id);
            Ok(())
        })
    }
}

impl Drop for RemoteWorkerCapacityLease {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

/// Atomically reserves one slot across all controllers using this state root.
/// Worker identity is pinned to the enrolled SSH host key to prevent stale
/// reservations being inherited by a rebound worker ID.
pub fn reserve_capacity(
    worker_id: &str,
    host_key_fingerprint: &str,
    max_jobs: u32,
    observed_running_jobs: u32,
) -> anyhow::Result<RemoteWorkerCapacityLease> {
    reserve_capacity_at(
        &crate::persistence::state_root(),
        worker_id,
        host_key_fingerprint,
        max_jobs,
        observed_running_jobs,
        now_unix_ms(),
    )
}

fn reserve_capacity_at(
    root: &Path,
    worker_id: &str,
    host_key_fingerprint: &str,
    max_jobs: u32,
    observed_running_jobs: u32,
    now: u64,
) -> anyhow::Result<RemoteWorkerCapacityLease> {
    anyhow::ensure!(
        !worker_id.is_empty() && worker_id.len() <= 128,
        "remote worker ID is invalid"
    );
    anyhow::ensure!((1..=256).contains(&max_jobs), "worker job limit is invalid");
    anyhow::ensure!(
        host_key_fingerprint.starts_with("SHA256:") && host_key_fingerprint.len() == 50,
        "worker host identity is invalid"
    );
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reservation = Reservation {
        id,
        worker_id: worker_id.to_owned(),
        host_key_fingerprint: host_key_fingerprint.to_owned(),
        expires_at_unix_ms: now.saturating_add(ttl_ms()),
        pending_start: true,
    };
    with_store(root, |store| {
        prune(store, now);
        let active = store
            .reservations
            .iter()
            .filter(|item| {
                item.worker_id == worker_id
                    && item.host_key_fingerprint == host_key_fingerprint
                    && item.pending_start
            })
            .count();
        anyhow::ensure!(
            observed_running_jobs.saturating_add(active as u32) < max_jobs,
            "remote worker has no unreserved execution slots"
        );
        store.reservations.push(reservation.clone());
        Ok(())
    })?;
    Ok(RemoteWorkerCapacityLease {
        root: root.to_path_buf(),
        reservation,
    })
}

fn with_store<T>(
    root: &Path,
    update: impl FnOnce(&mut Store) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let process_lock = PROCESS_STORE_LOCK.get_or_init(|| Mutex::new(()));
    let _process_guard = process_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    fs::create_dir_all(root)?;
    let lock_path = root.join(LOCK);
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path)?;
    let lock_metadata = fs::symlink_metadata(&lock_path)?;
    anyhow::ensure!(
        lock_metadata.is_file() && !lock_metadata.file_type().is_symlink(),
        "remote worker capacity lock is not a regular file"
    );
    fs2::FileExt::lock_exclusive(&lock)?;
    let path = root.join(STORE);
    let mut store = read_store(&path)?;
    let result = update(&mut store)?;
    write_store(&path, &store)?;
    fs2::FileExt::unlock(&lock)?;
    Ok(result)
}

fn read_store(path: &Path) -> anyhow::Result<Store> {
    let file = match open_store(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Store {
                schema_version: 1,
                reservations: Vec::new(),
                last_auto_worker_id: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        file.metadata()?.len() <= MAX_STORE_BYTES,
        "capacity store is too large"
    );
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes)?;
    let store: Store = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        store.schema_version == 1,
        "capacity store version is unsupported"
    );
    anyhow::ensure!(
        store.reservations.len() <= 16_384,
        "capacity store has too many reservations"
    );
    Ok(store)
}

fn open_store(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn write_store(path: &Path, store: &Store) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(store)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_STORE_BYTES,
        "capacity store is too large"
    );
    crate::artifacts::atomic_write_bytes(path, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn prune(store: &mut Store, now: u64) {
    store
        .reservations
        .retain(|item| item.expires_at_unix_ms > now);
}

fn ttl_ms() -> u64 {
    LEASE_TTL.as_millis().min(u64::MAX as u128) as u64
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "capacity/tests.rs"]
mod tests;
