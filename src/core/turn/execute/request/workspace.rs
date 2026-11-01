use super::*;

pub(super) fn bind(
    state: &PlannerState,
    task: Option<&str>,
    started: Instant,
) -> Result<Option<crate::execution::contract::WorkspaceRef>, Box<TurnOutcome>> {
    let Some(key) = task else {
        return Ok(None);
    };
    if crate::core::task_conversation::is_opportunity_conversation_key(key) {
        return Ok(None);
    }
    let work = crate::core::planning_work::find_card(state, key);
    let needs_source = work.as_ref().is_some_and(|work| {
        matches!(
            work.kind,
            crate::core::planning_work::WorkKind::Bug
                | crate::core::planning_work::WorkKind::Question
                | crate::core::planning_work::WorkKind::DocumentationRefresh
        )
    });
    if !needs_source {
        return Ok(None);
    }
    let task_id = state
        .items
        .iter()
        .find(|item| item.conversation_key() == key)
        .and_then(|item| item.uid.clone())
        .or_else(|| work.map(|work| work.uid))
        .unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string());
    let workspace = crate::core::source_workspace::bind(state, &task_id);
    if state.repo_root.join(".git").exists()
        && workspace.is_none()
        && !crate::core::source_workspace::is_unborn(state)
    {
        return Err(Box::new(TurnOutcome::HarnessFailed {
            error: AppError::Other("Cannot bind repository investigation to a verified source HEAD and task identity. Repair the selected workspace before retrying the saved answer.".into()),
            elapsed: started.elapsed(),
        }));
    }
    Ok(workspace)
}
