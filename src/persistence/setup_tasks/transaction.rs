use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use fs2::FileExt;

use super::{MachineSnapshot, Store, load_from, path, save_to};

const LOCK_FILE: &str = "tasks.lock";
static PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) fn reconcile_live(snapshot: &MachineSnapshot) -> anyhow::Result<Store> {
    reconcile_at(&path(), snapshot)
}

pub(super) fn reconcile_live_if(
    snapshot: &MachineSnapshot,
    still_current: impl Fn() -> bool,
) -> anyhow::Result<Option<Store>> {
    reconcile_at_if(&path(), snapshot, still_current)
}

fn reconcile_at(path: &Path, snapshot: &MachineSnapshot) -> anyhow::Result<Store> {
    reconcile_at_if(path, snapshot, || true)?
        .ok_or_else(|| anyhow::anyhow!("setup reconciliation was canceled"))
}

pub(crate) fn reconcile_at_if(
    path: &Path,
    snapshot: &MachineSnapshot,
    still_current: impl Fn() -> bool,
) -> anyhow::Result<Option<Store>> {
    snapshot.validate()?;
    let process_lock = PROCESS_LOCK.get_or_init(|| Mutex::new(()));
    let _process_guard = process_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing setup state directory"))?;
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "unsafe setup state directory"
        );
    }
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let lock_path = parent.join(LOCK_FILE);
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path)?;
    let metadata = fs::symlink_metadata(&lock_path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "unsafe setup state lock"
    );
    if !still_current() {
        return Ok(None);
    }
    #[cfg(test)]
    signal_lock_attempt()?;
    if !lock_exclusive_while_current(&lock, &still_current)? {
        return Ok(None);
    }

    let result = (|| {
        let mut store = load_from(path)?;
        if !still_current() {
            return Ok(None);
        }
        let changed = store.reconcile(snapshot)?;
        if !still_current() {
            return Ok(None);
        }
        if changed {
            save_to(path, &store)?;
        }
        Ok(Some(store))
    })();
    let unlock = FileExt::unlock(&lock);
    match (result, unlock) {
        (Ok(store), Ok(())) => Ok(store),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

fn lock_exclusive_while_current(
    lock: &std::fs::File,
    still_current: &impl Fn() -> bool,
) -> anyhow::Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if !still_current() {
            return Ok(false);
        }
        match FileExt::try_lock_exclusive(lock) {
            Ok(()) => return Ok(true),
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

#[cfg(test)]
mod tests;
