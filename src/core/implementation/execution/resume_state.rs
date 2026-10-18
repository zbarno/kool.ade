use super::super::*;
use fs2::FileExt;
use std::io::Read;

const MAX_INTERRUPTED_RESUMES: u32 = 3;

#[cfg(test)]
pub(crate) fn mark_resume_started(
    repo: &Path,
    ticket: &str,
) -> anyhow::Result<Option<Implementation>> {
    mark_resume_started_for_action(repo, ticket, true)
}

pub(crate) fn mark_resume_started_for_action(
    repo: &Path,
    ticket: &str,
    explicit_user_action: bool,
) -> anyhow::Result<Option<Implementation>> {
    let Some(initial) = super::super::load(repo, ticket) else {
        return Ok(None);
    };
    let dir = super::super::state_dir_for_task(repo, ticket, initial.task_uid.as_deref())?;
    let lock = open_regular_state_file(&dir.join("interrupted-resume.lock"), true)?
        .expect("created state lock is present");
    lock.lock_exclusive()?;
    let Some(mut state) = super::super::load(repo, ticket) else {
        return Ok(None);
    };
    if !matches!(
        state.status,
        ImplementationStatus::Blocked | ImplementationStatus::Interrupted
    ) {
        return Ok(Some(state));
    }
    let dir = super::super::state_dir_for_task(repo, ticket, state.task_uid.as_deref())?;
    let budget = read_interrupted_resume_budget(&dir)?;
    let interrupted_retry = state.status == ImplementationStatus::Interrupted || budget.is_some();
    anyhow::ensure!(
        explicit_user_action || !interrupted_retry,
        "Automatic queue retry is disabled for interrupted tasks; use Resume to review and retry the preserved workspace."
    );
    if explicit_user_action && interrupted_retry {
        increment_interrupted_resume_budget(&dir, budget)?;
    }
    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let history_name = format!("{stamp}-resume-context.txt");
    crate::artifacts::atomic_write(&dir.join(&history_name), &state.detail)?;
    let previous = crate::core::context_build::clip(&state.detail, 3000);
    state.status = ImplementationStatus::Preparing;
    state.detail = format!(
        "Resume accepted; the previous failure is preserved in {history_name}.\nPrevious failure detail:\n{previous}"
    );
    super::super::save(&dir, &state)?;
    Ok(Some(state))
}

fn read_interrupted_resume_budget(dir: &Path) -> anyhow::Result<Option<u32>> {
    let path = dir.join("interrupted-resumes.json");
    let Some(file) = open_regular_state_file(&path, false)? else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.len() <= 16,
        "Interrupted retry history is too large"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(17).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16, "Interrupted retry history is too large");
    serde_json::from_slice::<u32>(&bytes)
        .map(Some)
        .map_err(|error| anyhow::anyhow!("Interrupted retry history is invalid: {error}"))
}

fn increment_interrupted_resume_budget(dir: &Path, previous: Option<u32>) -> anyhow::Result<()> {
    let path = dir.join("interrupted-resumes.json");
    let count = previous.unwrap_or_default();
    anyhow::ensure!(
        count < MAX_INTERRUPTED_RESUMES,
        "Interrupted task retry limit ({MAX_INTERRUPTED_RESUMES}) reached; its workspace is preserved for inspection."
    );
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&(count + 1))?)?;
    Ok(())
}

fn open_regular_state_file(path: &Path, create: bool) -> anyhow::Result<Option<std::fs::File>> {
    use std::fs::OpenOptions;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Interrupted retry state is not a regular file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Interrupted retry state is not a regular file"
    );
    Ok(Some(file))
}

pub(crate) fn record_failed_attempt(
    repo: &Path,
    ticket: &str,
    detail: &str,
) -> anyhow::Result<Option<Implementation>> {
    let Some(mut state) = super::super::load(repo, ticket) else {
        return Ok(None);
    };
    state.status = ImplementationStatus::Blocked;
    state.detail = detail.to_owned();
    let dir = super::super::state_dir_for_task(repo, ticket, state.task_uid.as_deref())?;
    super::super::save(&dir, &state)?;
    Ok(Some(state))
}
