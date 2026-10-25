use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use fs2::FileExt;

struct InstanceLease {
    id: String,
    _file: File,
}

static LEASES: OnceLock<Mutex<BTreeMap<PathBuf, InstanceLease>>> = OnceLock::new();

/// Acquire and retain this process's owner lease for the store directory.
pub(super) fn ensure_current(state_path: &Path) -> anyhow::Result<String> {
    let directory = state_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing setup state directory"))?;
    let leases = LEASES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut leases = leases.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(lease) = leases.get(directory) {
        return Ok(lease.id.clone());
    }

    ensure_directory(directory)?;
    let instances = directory.join("instances");
    ensure_directory(&instances)?;
    let id = uuid::Uuid::new_v4().to_string();
    let file = open_lock_file(&instances.join(format!("{id}.lock")), true)?;
    file.try_lock_exclusive()?;
    leases.insert(
        directory.to_path_buf(),
        InstanceLease {
            id: id.clone(),
            _file: file,
        },
    );
    Ok(id)
}

/// A held owner lock means another live app instance still owns the attempt.
/// Errors fail closed so a questionable lock path cannot interrupt that owner.
pub(super) fn is_active(state_path: &Path, id: &str) -> bool {
    if uuid::Uuid::parse_str(id).is_err() {
        return false;
    }
    let Some(directory) = state_path.parent() else {
        return true;
    };
    let instances = directory.join("instances");
    match fs::symlink_metadata(&instances) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        _ => return true,
    }
    let path = instances.join(format!("{id}.lock"));
    let file = match open_lock_file(&path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            drop(file);
            let _ = fs::remove_file(path);
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Err(_) => true,
    }
}

fn ensure_directory(path: &Path) -> anyhow::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "unsafe setup owner directory"
        );
    }
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "unsafe setup owner directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open_lock_file(path: &Path, create: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "unsafe setup owner lock",
        ));
    }
    Ok(file)
}
