use super::PrivateDirectory;
use fs2::FileExt;
use std::{fs, io, path::Path};
mod locks;

use locks::{
    create_lock_file, ensure_private_root, is_private_directory, open_existing_lock,
    open_lock_file, process_is_alive, verify_lock_file,
};

const SESSION_PREFIX: &str = "koolade-cli-session";
const SESSION_ROOT: &str = "s";
const LEGACY_SESSION_ROOT: &str = "koolade-cli-sessions";
const COORDINATOR_LOCK: &str = ".recovery.lock";
const ACTIVE_LOCK: &str = ".active.lock";

pub(super) fn create_session(temp_root: &Path) -> anyhow::Result<PrivateDirectory> {
    #[cfg(not(test))]
    {
        let legacy_temp_root = std::env::temp_dir();
        if legacy_temp_root != temp_root {
            let _ = reclaim_legacy_sessions(&legacy_temp_root);
            let _ = reclaim_legacy_session_root(&legacy_temp_root);
        }
    }
    let session_root = temp_root.join(SESSION_ROOT);
    ensure_private_root(&session_root)?;
    let coordinator = open_lock_file(&session_root.join(COORDINATOR_LOCK))?;
    verify_lock_file(&session_root.join(COORDINATOR_LOCK), &coordinator)?;
    FileExt::lock_exclusive(&coordinator)?;
    let result = create_session_locked(temp_root, &session_root);
    let _ = FileExt::unlock(&coordinator);
    result
}

/// Drop shares creation/recovery's coordinator, keeping its active lease until
/// cleanup is admitted. If admission fails, leave the directory for recovery.
pub(super) fn release_session(path: &Path, active_lock: fs::File) {
    let Some(root) = path.parent() else {
        return;
    };
    let coordinator_path = root.join(COORDINATOR_LOCK);
    let Ok(coordinator) = open_existing_lock(&coordinator_path) else {
        return;
    };
    if verify_lock_file(&coordinator_path, &coordinator).is_err()
        || FileExt::lock_exclusive(&coordinator).is_err()
    {
        return;
    }
    drop(active_lock);
    let _ = fs::remove_dir_all(path);
    let _ = FileExt::unlock(&coordinator);
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn create_session_locked(
    temp_root: &Path,
    session_root: &Path,
) -> anyhow::Result<PrivateDirectory> {
    reclaim_crashed_sessions(temp_root, session_root)?;
    let path = session_root.join(format!(
        "r-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..20]
    ));
    super::create_private_directory(&path)?;
    if let Err(error) = super::verify_private_acl(&path) {
        let _ = fs::remove_dir_all(&path);
        return Err(error);
    }
    let active_path = path.join(ACTIVE_LOCK);
    let active_lock = match create_lock_file(&active_path) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_dir_all(&path);
            return Err(error.into());
        }
    };
    if let Err(error) = verify_lock_file(&active_path, &active_lock)
        .and_then(|()| FileExt::lock_exclusive(&active_lock).map_err(Into::into))
    {
        drop(active_lock);
        let _ = fs::remove_dir_all(&path);
        return Err(error);
    }
    Ok(PrivateDirectory {
        path,
        active_lock: Some(active_lock),
    })
}

fn reclaim_crashed_sessions(temp_root: &Path, session_root: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(session_root)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == COORDINATOR_LOCK || !is_session_run_name(&name) {
            continue;
        }
        if !is_private_directory(&path)? {
            continue;
        }
        let lock_path = path.join(ACTIVE_LOCK);
        match open_existing_lock(&lock_path) {
            Ok(lock) => match FileExt::try_lock_exclusive(&lock) {
                Ok(()) => {
                    FileExt::unlock(&lock)?;
                    drop(lock);
                    remove_if_present(&path)?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Creation and recovery share the coordinator lock, so no live
                // session can be between directory creation and lock creation.
                remove_if_present(&path)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    reclaim_legacy_sessions(temp_root)
}

fn is_session_run_name(name: &str) -> bool {
    name.strip_prefix("r-")
        .is_some_and(|id| id.len() == 20 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn reclaim_legacy_session_root(temp_root: &Path) -> anyhow::Result<()> {
    let root = temp_root.join(LEGACY_SESSION_ROOT);
    if !matches!(is_private_directory(&root), Ok(true)) {
        return Ok(());
    }
    let coordinator_path = root.join(COORDINATOR_LOCK);
    let coordinator = match open_existing_lock(&coordinator_path) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    verify_lock_file(&coordinator_path, &coordinator)?;
    match FileExt::try_lock_exclusive(&coordinator) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == COORDINATOR_LOCK || parse_run_name(&name, "run-").is_none() {
            continue;
        }
        let path = entry.path();
        if !is_private_directory(&path)? {
            continue;
        }
        let active_path = path.join(ACTIVE_LOCK);
        match open_existing_lock(&active_path) {
            Ok(active) => match FileExt::try_lock_exclusive(&active) {
                Ok(()) => {
                    FileExt::unlock(&active)?;
                    drop(active);
                    remove_if_present(&path)?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                remove_if_present(&path)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let _ = FileExt::unlock(&coordinator);
    Ok(())
}

fn reclaim_legacy_sessions(temp_root: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(temp_root)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((pid, _)) = parse_run_name(&name, &format!("{SESSION_PREFIX}-")) else {
            continue;
        };
        let path = entry.path();
        if !matches!(is_private_directory(&path), Ok(true)) || process_is_alive(pid) {
            continue;
        }
        remove_if_present(&path)?;
    }
    Ok(())
}

fn parse_run_name(name: &str, prefix: &str) -> Option<(u32, uuid::Uuid)> {
    let suffix = name.strip_prefix(prefix)?;
    let (pid, id) = suffix.split_once('-')?;
    Some((pid.parse().ok()?, uuid::Uuid::parse_str(id).ok()?))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod drop_tests;
