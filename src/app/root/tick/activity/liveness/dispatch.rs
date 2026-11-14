use crate::{
    app::session::Project,
    core::{
        lifecycle::{TaskPhase, TaskRecord},
        planning_work::WorkStatus,
    },
};

pub(super) const MAX_AUTOMATIC_ATTEMPTS: u8 = 2;

pub(super) fn has_dispatchable_task(
    project: &Project,
    tasks: &[&TaskRecord],
) -> Result<bool, String> {
    for task in tasks {
        let dispatchable = match task.phase {
            TaskPhase::Ready
            | TaskPhase::AwaitingReview
            | TaskPhase::Repairing
            | TaskPhase::ReReview
            | TaskPhase::AwaitingRuntimeQa
            | TaskPhase::AwaitingSpecReconciliation => true,
            TaskPhase::Running => running_task_has_owner(project, task)?,
            TaskPhase::Blocked
            | TaskPhase::Done
            | TaskPhase::PrPending
            | TaskPhase::PrOpened
            | TaskPhase::PrMerged
            | TaskPhase::Failed
            | TaskPhase::Cancelled => false,
        };
        if dispatchable {
            return Ok(true);
        }
    }
    Ok(false)
}

fn running_task_has_owner(project: &Project, task: &TaskRecord) -> Result<bool, String> {
    let ticket = project
        .task_documents
        .iter()
        .find(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == task.task_uid)
        })
        .map(|document| document.path.as_str());
    if ticket.is_some_and(|ticket| {
        project.active_implementations.contains_key(ticket)
            || project.queue.in_flight.contains(ticket)
    }) {
        return Ok(true);
    }
    crate::core::task_claim::is_claimed_by_another_process(
        &project.state.planning_store.root,
        project.state.planning_store.project_id,
        &task.task_uid,
    )
    .map_err(|error| format!("Could not verify the running task lease: {error}"))
}

#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;

/// Claim one durable recovery card and hand it to the ordinary scoped Planner.
pub(in crate::app::root::tick::activity) fn next_recovery(
    project: &mut Project,
) -> Option<(String, String, Option<String>)> {
    if project.active_turn.is_some()
        || project.active_planning_work.is_some()
        || project.activity.manager.is_some()
    {
        return None;
    }
    let index = project.planning_work.iter().position(|work| {
        work.key.starts_with("task:manager-liveness:")
            && work.status == WorkStatus::Todo
            && work.liveness_recovery_attempts < MAX_AUTOMATIC_ATTEMPTS
    })?;
    let previous = project.planning_work[index].clone();
    let recovery = &mut project.planning_work[index];
    recovery.liveness_recovery_attempts = recovery.liveness_recovery_attempts.saturating_add(1);
    recovery.status = WorkStatus::InProgress;
    recovery.detail.push_str(&format!(
        "\n\nPlanner recovery attempt {} of {} was durably claimed for automatic dispatch.",
        recovery.liveness_recovery_attempts, MAX_AUTOMATIC_ATTEMPTS
    ));
    let selection = (
        recovery.key.clone(),
        recovery.request.clone(),
        recovery.feature_id.clone(),
    );
    if let Err(error) = project.save_planning_work() {
        if let Some(current) = project
            .planning_work
            .iter_mut()
            .find(|work| work.uid == previous.uid)
        {
            *current = previous;
        }
        let notice =
            format!("TaskIntent recovery dispatch could not be persisted and will retry: {error}");
        if !project.activity.pending.iter().any(|pending| {
            pending.starts_with("TaskIntent recovery dispatch could not be persisted")
        }) {
            project.activity.pending.push(notice);
        }
        return None;
    }
    Some(selection)
}
