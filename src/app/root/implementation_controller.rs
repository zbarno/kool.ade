//! Task queue scheduling and implementation lifecycle orchestration.
use super::*;
mod auto_queue;
#[cfg(test)]
mod tests;

fn lifecycle_parent_contains_planning_child(
    child_planning_uid: &str,
    parent_planning_uid: &str,
    child: &crate::core::lifecycle::TaskRecord,
    parent: &crate::core::lifecycle::TaskRecord,
) -> bool {
    child.planning_item_uid.as_deref() == Some(child_planning_uid)
        && child.parent_task_uid.as_deref() == Some(parent.task_uid.as_str())
        && parent.planning_item_uid.as_deref() == Some(parent_planning_uid)
        && parent.subtask_uids.contains(&child.task_uid)
}

pub(super) fn completed_subtask_uids(
    project: &crate::app::session::Project,
) -> std::collections::BTreeSet<String> {
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    project
        .task_documents
        .iter()
        .filter_map(|document| document.metadata.as_ref())
        .filter(|metadata| metadata.parent_task_uid.is_some())
        .filter_map(|metadata| {
            let implementation = project
                .task_documents
                .iter()
                .find(|document| {
                    document
                        .metadata
                        .as_ref()
                        .is_some_and(|candidate| candidate.uid == metadata.uid)
                })
                .and_then(|document| project.implementation_states.get(&document.path))?;
            let task = lifecycle
                .task_for_planning_item(&metadata.uid)
                .ok()
                .flatten()?;
            let parent_planning_uid = metadata.parent_task_uid.as_deref()?;
            let parent = lifecycle
                .task_for_planning_item(parent_planning_uid)
                .ok()
                .flatten()?;
            (task.phase == crate::core::lifecycle::TaskPhase::Done
                && lifecycle_parent_contains_planning_child(
                    &metadata.uid,
                    parent_planning_uid,
                    &task,
                    &parent,
                )
                && implementation.status
                    == crate::core::implementation::ImplementationStatus::AwaitingApproval
                && implementation.pr_url.is_none()
                && implementation.verified_head.as_deref()
                    == task.reviewed_code_head_sha.as_deref())
            .then_some(metadata.uid.clone())
        })
        .collect()
}

impl KooladeApp {
    pub(super) fn advance_auto_publish(&mut self) {
        let ready = match &self.screen {
            Screen::Connected(project)
                if project.queue.auto_publish
                    && project.active_implementations.len()
                        < project.queue.max_parallel.clamp(1, 8) =>
            {
                project
                    .implementation_states
                    .iter()
                    .filter(|(ticket, state)| {
                        state.status == ImplementationStatus::ReadyToPublish
                            && !project.task_cancelled(ticket)
                            && !project.active_implementations.contains_key(*ticket)
                            && !project.queue.blocked.contains_key(*ticket)
                    })
                    .map(|(ticket, _)| ticket.clone())
                    .collect::<Vec<_>>()
            }
            _ => return,
        };
        for ticket in ready {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            if project.active_implementations.len() >= project.queue.max_parallel.clamp(1, 8) {
                break;
            }
            let failure = crate::core::implementation::Failure::new(
                crate::core::implementation::FailureKind::ExternalPrerequisite,
                crate::core::implementation::RecoveryDisposition::UserAction,
                "Verified work is saved locally. Review it, then choose Share verified work for review before Kool.ad/e pushes a branch or creates a pull request.",
            );
            project.queue.blocked.insert(ticket.clone(), failure);
            project.activity.pending.push(format!(
                "{ticket} needs attention: verified work is ready for review before it can be shared."
            ));
            if let Err(error) = project.queue.save(&project.state.repo_root) {
                project.queue.last_error =
                    format!("Cannot save the publication review item for {ticket}: {error}");
                return;
            }
        }
    }
}

mod start;
