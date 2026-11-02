use super::PrivateDirectory;
use fs2::FileExt;
use std::{fs, io, path::Path};
mod locks;

use locks::{
    create_lock_file, ensure_private_root, is_private_directory, open_existing_lock,
    open_lock_file, process_is_alive, verify_lock_file,
};

const SESSION_PREFIX: &str = "koolade-cli-session";
const SESSION_ROOT: &str = "koolade-cli-sessions";
const COORDINATOR_LOCK: &str = ".recovery.lock";
const ACTIVE_LOCK: &str = ".active.lock";

pub(super) fn create_session(temp_root: &Path) -> anyhow::Result<PrivateDirectory> {
    let session_root = temp_root.join(SESSION_ROOT);
    ensure_private_root(&session_root)?;
    let coordinator = open_lock_file(&session_root.join(COORDINATOR_LOCK))?;
    verify_lock_file(&session_root.join(COORDINATOR_LOCK), &coordinator)?;
    FileExt::lock_exclusive(&coordinator)?;
    let result = create_session_locked(temp_root, &session_root);
    let _ = FileExt::unlock(&coordinator);
    result
}

fn create_session_locked(
    temp_root: &Path,
    session_root: &Path,
) -> anyhow::Result<PrivateDirectory> {
    reclaim_crashed_sessions(temp_root, session_root)?;
    let path = session_root.join(format!(
        "run-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
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
        if name == COORDINATOR_LOCK || !parse_run_name(&name, "run-").is_some() {
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
                    fs::remove_dir_all(path)?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Creation and recovery share the coordinator lock, so no live
                // session can be between directory creation and lock creation.
                fs::remove_dir_all(path)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    reclaim_legacy_sessions(temp_root)
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
        if !is_private_directory(&path)? || process_is_alive(pid) {
            continue;
        }
        fs::remove_dir_all(path)?;
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
