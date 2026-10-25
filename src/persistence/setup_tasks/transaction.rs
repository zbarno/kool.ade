use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use fs2::FileExt;

use super::{MAX_STATE_BYTES, SetupTaskStoreV1, migration};

mod owner;
#[cfg(test)]
mod tests;
mod transitions;

pub use transitions::apply_transition;
#[cfg(test)]
pub(super) use transitions::apply_transition_at;
const LOCK_FILE: &str = "tasks.lock";
static PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) fn path() -> std::path::PathBuf {
    path_for_state_root(&crate::persistence::state_root())
}

pub(super) fn path_for_state_root(root: &Path) -> std::path::PathBuf {
    root.join("machine-setup").join("tasks.json")
}

pub fn load() -> anyhow::Result<SetupTaskStoreV1> {
    update_at(&path(), |store| Ok((store.clone(), false)))
}

/// Persist a monotonic scan revision before probes start. A later scan fences
/// out an older probe result when it reaches `reconcile_live`.
pub fn begin_scan() -> anyhow::Result<u64> {
    begin_scan_at(&path())
}

pub(crate) fn begin_scan_at(path: &Path) -> anyhow::Result<u64> {
    update_at(path, |store| {
        store.latest_scan_generation = store
            .latest_scan_generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("setup scan generation overflow"))?;
        Ok((store.latest_scan_generation, true))
    })
}

pub fn reconcile_live(
    snapshot: &crate::app::setup_attention::catalog::MachineSnapshot,
    generation: u64,
) -> anyhow::Result<SetupTaskStoreV1> {
    reconcile_live_at(&path(), snapshot, generation)
}

pub(super) fn reconcile_live_at(
    path: &Path,
    snapshot: &crate::app::setup_attention::catalog::MachineSnapshot,
    generation: u64,
) -> anyhow::Result<SetupTaskStoreV1> {
    reconcile_at_if(path, snapshot, generation, || true)?
        .ok_or_else(|| anyhow::anyhow!("setup reconciliation was canceled or stale"))
}

pub(crate) fn reconcile_live_if(
    snapshot: &crate::app::setup_attention::catalog::MachineSnapshot,
    generation: u64,
    still_current: impl Fn() -> bool,
) -> anyhow::Result<Option<SetupTaskStoreV1>> {
    reconcile_at_if(&path(), snapshot, generation, still_current)
}

pub(crate) fn reconcile_at_if(
    path: &Path,
    snapshot: &crate::app::setup_attention::catalog::MachineSnapshot,
    generation: u64,
    still_current: impl Fn() -> bool,
) -> anyhow::Result<Option<SetupTaskStoreV1>> {
    if !still_current() {
        return Ok(None);
    }
    let current_owner = owner::ensure_current(path)?;
    update_at(path, |store| {
        let recovered = store.recover_interrupted(
            crate::persistence::harness_settings::current_unix_ms(),
            &current_owner,
            |id| owner::is_active(path, id),
        );
        if !still_current() || generation < store.latest_scan_generation {
            return Ok((None, recovered));
        }
        anyhow::ensure!(
            generation == store.latest_scan_generation,
            "unreserved setup scan generation"
        );
        let mut candidate = store.clone();
        let changed = candidate.reconcile(snapshot, generation)?;
        if !still_current() {
            return Ok((None, recovered));
        }
        *store = candidate.clone();
        Ok((Some(candidate), recovered || changed))
    })
}

/// Called exactly once at application startup. It cancels stale consent and
/// marks operations interrupted; no mutation is resumed or replayed.
pub fn recover_after_restart() -> anyhow::Result<SetupTaskStoreV1> {
    recover_interrupted_at(
        &path(),
        crate::persistence::harness_settings::current_unix_ms(),
    )
}

pub(crate) fn recover_interrupted_at(
    path: &Path,
    at_unix_ms: u64,
) -> anyhow::Result<SetupTaskStoreV1> {
    let current_owner = owner::ensure_current(path)?;
    update_at(path, |store| {
        let changed =
            store.recover_interrupted(at_unix_ms, &current_owner, |id| owner::is_active(path, id));
        Ok((store.clone(), changed))
    })
}

pub(super) fn update_at<T>(
    path: &Path,
    update: impl FnOnce(&mut SetupTaskStoreV1) -> anyhow::Result<(T, bool)>,
) -> anyhow::Result<T> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing setup state directory"))?;
    ensure_private_directory(parent)?;
    let process_lock = PROCESS_LOCK.get_or_init(|| Mutex::new(()));
    let _process_guard = process_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let lock = open_lock_file(parent)?;
    #[cfg(test)]
    signal_lock_attempt()?;
    lock_exclusive(&lock)?;

    let result = (|| {
        let (mut store, migrated) = load_from(path)?;
        let (value, changed) = update(&mut store)?;
        store.validate()?;
        if changed || migrated {
            save_to(path, &store)?;
        }
        Ok(value)
    })();
    let unlock = FileExt::unlock(&lock);
    match (result, unlock) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

fn ensure_private_directory(parent: &Path) -> anyhow::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "unsafe setup state directory"
        );
    }
    fs::create_dir_all(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "unsafe setup state directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open_lock_file(parent: &Path) -> anyhow::Result<File> {
    let lock_path = parent.join(LOCK_FILE);
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(&lock_path)?;
    let metadata = fs::symlink_metadata(&lock_path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "unsafe setup state lock"
    );
    Ok(file)
}

fn lock_exclusive(lock: &File) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match FileExt::try_lock_exclusive(lock) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::bail!("timed out waiting for the setup task state lock")
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
fn signal_lock_attempt() -> std::io::Result<()> {
    if let Some(path) = std::env::var_os("KOOLADE_SETUP_LOCK_ATTEMPT_MARKER") {
        fs::write(path, b"waiting")?;
    }
    Ok(())
}

pub(super) fn load_from(path: &Path) -> anyhow::Result<(SetupTaskStoreV1, bool)> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((SetupTaskStoreV1::default(), false));
        }
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "unsafe setup task store path"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_STATE_BYTES,
        "setup task state is oversized"
    );
    migration::decode(&fs::read(path)?)
}

pub(super) fn save_to(path: &Path, store: &SetupTaskStoreV1) -> anyhow::Result<()> {
    store.validate()?;
    let payload = serde_json::to_string_pretty(store)?;
    anyhow::ensure!(
        payload.len() <= MAX_STATE_BYTES as usize,
        "setup task state is oversized"
    );
    crate::artifacts::atomic_write(path, &payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
