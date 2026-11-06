use super::*;

mod persist;
#[cfg(not(test))]
use persist::trim_for_persist;
#[cfg(test)]
pub(super) use persist::{
    ACTIVITY_MAX_FIELD_CHARS, ACTIVITY_MAX_POST_CHARS, ACTIVITY_MAX_POSTS, retain_suffix,
    trim_for_persist,
};
#[cfg(test)]
#[path = "activity/tests.rs"]
mod tests;

/// Task-only activity survives reconnects without becoming a tracked artifact.
pub fn load_activity(repo: &Path, ticket: &str) -> Option<crate::harness::LiveProgress> {
    load_activity_for_task(repo, ticket, None)
}

pub fn load_activity_for_task(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
) -> Option<crate::harness::LiveProgress> {
    let path = activity_path_for_task(repo, ticket, task_uid).ok()?;
    let (mut stored, legacy_path) = match fs::read(&path) {
        Ok(bytes) => (serde_json::from_slice(&bytes).ok()?, None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy_path = legacy_activity_path(repo, ticket, task_uid).ok()?;
            let legacy: crate::harness::LiveProgress =
                serde_json::from_slice(&fs::read(&legacy_path).ok()?).ok()?;
            if let Some(uid) = task_uid {
                let state_proves_identity = super::state_paths::state_dir_by_task_uid(repo, uid)
                    .ok()?
                    .is_some();
                if legacy.task_uid.as_deref() != Some(uid) && !state_proves_identity {
                    return None;
                }
                if legacy
                    .task_uid
                    .as_deref()
                    .is_some_and(|stored| stored != uid)
                {
                    return None;
                }
                let mut legacy = legacy;
                legacy.task_uid = Some(uid.to_owned());
                (legacy, Some(legacy_path))
            } else {
                (legacy, None)
            }
        }
        Err(_) => return None,
    };
    if let Some(uid) = task_uid
        && stored
            .task_uid
            .as_deref()
            .is_some_and(|stored| stored != uid)
    {
        return None;
    }
    if let Some(uid) = task_uid {
        stored.task_uid = Some(uid.to_owned());
    }
    let safe = trim_for_persist(&stored);
    if let Some(legacy_path) = legacy_path {
        save_activity(repo, ticket, &safe).ok()?;
        let safe_bytes = serde_json::to_vec(&safe).ok()?;
        crate::artifacts::atomic_write_bytes(&legacy_path, &safe_bytes).ok()?;
        fs::remove_file(legacy_path).ok()?;
    } else if safe != stored {
        save_activity(repo, ticket, &safe).ok()?;
    }
    Some(safe)
}
pub fn save_activity(
    repo: &Path,
    ticket: &str,
    activity: &crate::harness::LiveProgress,
) -> anyhow::Result<()> {
    let dir = activity_dir_for_task(repo, ticket, activity.task_uid.as_deref())?;
    fs::create_dir_all(&dir)?;
    // Persist a bounded view: in-memory snapshots keep the full history, but
    // the on-disk snapshot keeps only trailing windows so a long turn cannot
    // bloat activity.json (quadratic rewrites of ever-larger files were a
    // major source of disk wear).
    crate::artifacts::atomic_write_bytes(
        &dir.join("activity.json"),
        &serde_json::to_vec(&trim_for_persist(activity))?,
    )
}

fn activity_dir_for_task(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let Some(uid) = task_uid else {
        return state_dir_for_task(repo, ticket, None);
    };
    let base =
        super::state_paths::state_dir_by_task_uid(repo, uid)?.unwrap_or(state_dir(repo, ticket)?);
    use sha2::Digest;
    let uid_hash = sha2::Sha256::digest(uid.as_bytes());
    let uid_key = uid_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(base.join("activity-history").join(uid_key))
}

fn activity_path_for_task(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
) -> anyhow::Result<PathBuf> {
    Ok(activity_dir_for_task(repo, ticket, task_uid)?.join("activity.json"))
}

fn legacy_activity_path(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let dir = if let Some(uid) = task_uid {
        super::state_paths::state_dir_by_task_uid(repo, uid)?.unwrap_or(state_dir(repo, ticket)?)
    } else {
        state_dir_for_task(repo, ticket, None)?
    };
    Ok(dir.join("activity.json"))
}

/// Close a persisted activity snapshot after its implementation record has
/// reached a terminal state and its worker no longer holds the task lock.
/// This repairs snapshots left unfinished when a window disconnects before it
/// can consume the worker's final event.
pub fn finalize_terminal_activity_if_stale(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
    status: super::ImplementationStatus,
    progress: &mut crate::harness::LiveProgress,
    finished_ms: i64,
) -> bool {
    if !matches!(
        status,
        super::ImplementationStatus::Blocked
            | super::ImplementationStatus::Interrupted
            | super::ImplementationStatus::ReadyToPublish
            | super::ImplementationStatus::AwaitingApproval
            | super::ImplementationStatus::ChangesRequested
            | super::ImplementationStatus::AwaitingReview
            | super::ImplementationStatus::Completed
            | super::ImplementationStatus::PullRequestClosed
    ) || progress.telemetry.finished_ms.is_some()
        || !has_activity(progress)
        || worker_holds_lock(repo, ticket, task_uid)
    {
        return false;
    }
    progress.telemetry.updated_ms = Some(finished_ms);
    progress.telemetry.finished_ms = Some(finished_ms);
    progress.activity = Some(status.label().to_owned());
    true
}

fn has_activity(progress: &crate::harness::LiveProgress) -> bool {
    progress.telemetry.started_ms.is_some()
        || progress.telemetry.updated_ms.is_some()
        || progress.telemetry.updates > 0
        || progress.activity.is_some()
        || !progress.posts.is_empty()
        || !progress.thoughts.is_empty()
        || !progress.response.is_empty()
}

fn worker_holds_lock(repo: &Path, ticket: &str, task_uid: Option<&str>) -> bool {
    let Ok(dir) = super::state_dir_for_task(repo, ticket, task_uid) else {
        return true;
    };
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    if file.try_lock().is_err() {
        return true;
    }
    let _ = file.unlock();
    false
}
