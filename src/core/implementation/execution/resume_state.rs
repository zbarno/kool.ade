use super::super::*;

pub(crate) fn mark_resume_started(
    repo: &Path,
    ticket: &str,
) -> anyhow::Result<Option<Implementation>> {
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
