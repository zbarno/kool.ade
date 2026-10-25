use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use fs2::FileExt;

use super::{permissions, root::Roots};
mod copy;
use copy::{copy_tree, digest_tree, remove_tree_no_links};
mod receipt;

const STAGING: &str = ".machine-setup-migration-staging";
static LEGACY_LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, File>>> = OnceLock::new();

pub(super) fn resolve(roots: &Roots) -> anyhow::Result<PathBuf> {
    let destination_root = &roots.app_data_root;
    permissions::ensure_private_directory(destination_root)?;
    let destination = destination_root.join("machine-setup");
    let legacy_root = roots
        .legacy_state_root
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("legacy machine setup state path is unavailable"))?;
    permissions::ensure_private_directory(legacy_root)?;
    let legacy = legacy_root.join("machine-setup");
    permissions::ensure_private_directory(&legacy)?;
    if has_retained_lock(&legacy) {
        return resolve_with_lock(roots, &destination, &legacy, false);
    }

    let has_receipt = match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !permissions::is_link_or_reparse(&metadata),
                "unsafe platform machine setup state path"
            );
            receipt::read(&destination)?.is_some()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if has_receipt {
        let lock = lock_legacy_shared(&legacy)?;
        let path = resolve_with_lock(roots, &destination, &legacy, false)?;
        retain_shared_lock(&legacy, lock);
        return Ok(path);
    }

    if let Some(lock) = try_lock_legacy_exclusive(&legacy)? {
        let path = resolve_with_lock(roots, &destination, &legacy, true)?;
        downgrade_legacy_lock(&lock)?;
        let verified = resolve_with_lock(roots, &destination, &legacy, false)?;
        anyhow::ensure!(path == verified);
        retain_shared_lock(&legacy, lock);
        return Ok(verified);
    }

    // Another new install may have completed migration and retained a shared
    // lock. Wait for that exclusive migration phase, then re-read its receipt.
    let lock = lock_legacy_shared(&legacy)?;
    let path = resolve_with_lock(roots, &destination, &legacy, false)?;
    retain_shared_lock(&legacy, lock);
    Ok(path)
}

fn resolve_with_lock(
    roots: &Roots,
    destination: &Path,
    legacy: &Path,
    exclusive: bool,
) -> anyhow::Result<PathBuf> {
    copy::ensure_no_active_owners(legacy)?;
    let legacy_digest = digest_tree(legacy)?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !permissions::is_link_or_reparse(&metadata),
                "unsafe platform machine setup state path"
            );
            permissions::ensure_private_directory(destination)?;
            if let Some(receipt) = receipt::read(destination)? {
                receipt::validate_legacy_snapshot(&receipt, &legacy_digest)?;
                return Ok(destination.join("tasks.json"));
            }
            anyhow::ensure!(
                exclusive,
                "platform machine setup migration receipt is unavailable"
            );
            anyhow::ensure!(
                legacy_digest.files == 0 && legacy_digest.bytes == 0,
                "platform and legacy machine setup stores both exist without a migration receipt; resolve the state conflict before continuing"
            );
            anyhow::ensure!(
                fs::read_dir(destination)?.next().is_none(),
                "platform machine setup store has data but no migration receipt"
            );
            receipt::write_fresh(destination)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::ensure!(
                exclusive,
                "platform machine setup migration receipt is unavailable"
            );
            if legacy_digest.files > 0 || legacy_digest.bytes > 0 {
                migrate(legacy, destination, &roots.app_data_root)?;
            } else {
                fs::create_dir(destination)?;
                permissions::ensure_private_directory(destination)?;
                receipt::write_fresh(destination)?;
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(destination.join("tasks.json"))
}

fn migrate(source: &Path, destination: &Path, destination_root: &Path) -> anyhow::Result<()> {
    copy::ensure_no_active_owners(source)?;
    let staging = destination_root.join(STAGING);
    if fs::symlink_metadata(&staging).is_ok() {
        remove_tree_no_links(&staging)?;
    }
    fs::create_dir(&staging)?;
    permissions::ensure_private_directory(&staging)?;

    let source_digest = copy_tree(source, &staging)?;
    let staged_digest = digest_tree(&staging)?;
    anyhow::ensure!(
        source_digest == staged_digest,
        "machine setup state changed or failed verification during migration"
    );
    receipt::write(
        &staging,
        &format!(
            "source=legacy-copy\nsha256={}\nfiles={}\nbytes={}\n",
            source_digest.hash, staged_digest.files, staged_digest.bytes
        ),
    )?;
    fs::rename(&staging, destination)?;
    permissions::ensure_private_directory(destination)?;
    Ok(())
}

fn has_retained_lock(directory: &Path) -> bool {
    let locks = LEGACY_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    locks
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains_key(directory)
}

fn retain_shared_lock(directory: &Path, file: File) {
    let locks = LEGACY_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = locks.lock().unwrap_or_else(|error| error.into_inner());
    locks.entry(directory.to_path_buf()).or_insert(file);
}

fn open_legacy_lock(directory: &Path) -> anyhow::Result<File> {
    let path = directory.join(crate::persistence::setup_tasks::TASK_STORE_LOCK);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(&path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !permissions::is_link_or_reparse(&metadata),
        "unsafe legacy machine setup task lock"
    );
    permissions::secure_private_file(&path)?;
    Ok(file)
}

fn try_lock_legacy_exclusive(directory: &Path) -> anyhow::Result<Option<File>> {
    let file = open_legacy_lock(directory)?;
    if wait_for_lock(&file, false)? {
        Ok(Some(file))
    } else {
        Ok(None)
    }
}

fn lock_legacy_shared(directory: &Path) -> anyhow::Result<File> {
    let file = open_legacy_lock(directory)?;
    anyhow::ensure!(
        wait_for_lock(&file, true)?,
        "timed out waiting for the legacy machine setup store lock"
    );
    Ok(file)
}

fn downgrade_legacy_lock(file: &File) -> anyhow::Result<()> {
    FileExt::unlock(file)?;
    anyhow::ensure!(
        wait_for_lock(file, true)?,
        "timed out downgrading the legacy machine setup store lock"
    );
    Ok(())
}

fn wait_for_lock(file: &File, shared: bool) -> anyhow::Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = if shared {
            FileExt::try_lock_shared(file)
        } else {
            FileExt::try_lock_exclusive(file)
        };
        match result {
            Ok(()) => return Ok(true),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        }
    }
}
