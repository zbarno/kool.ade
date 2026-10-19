use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
    sync::{Mutex, OnceLock},
};

pub(crate) const STORE: &str = "remote-worker-capacity.json";
const LOCK: &str = "remote-worker-capacity.lock";
const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;

// POSIX record locks are process scoped, so two independent handles in this
// process can otherwise both enter the read/modify/write section. Keep a
// process-local guard in addition to the file lock used across processes.
static PROCESS_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Reservation {
    pub(super) id: String,
    pub(super) worker_id: String,
    pub(super) host_key_fingerprint: String,
    pub(super) expires_at_unix_ms: u64,
    #[serde(default = "default_pending_start")]
    pub(super) pending_start: bool,
}

fn default_pending_start() -> bool {
    true
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Store {
    pub(super) schema_version: u8,
    pub(super) reservations: Vec<Reservation>,
    #[serde(default)]
    pub(super) last_auto_worker_id: Option<String>,
}

pub(super) fn with_store<T>(
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
