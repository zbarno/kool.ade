use super::super::request::PreparedRequest;
use super::super::{Project, event_uid, recipe_queue, request};
use crate::core::lifecycle::{LifecycleStore, QaExecutionStateV1, TaskRecord};

/// Start an approved recipe or form a new QA request for one candidate.
/// Returns true only when a recipe worker was started and the caller should
/// stop scanning candidates for this tick.
pub(super) fn process_without_request(
    project: &mut Project,
    task: &TaskRecord,
    qa_state: Option<&QaExecutionStateV1>,
    notices: &mut Vec<String>,
    launch: &mut Option<(PreparedRequest, String)>,
) -> bool {
    match recipe_queue::ready(project, task) {
        Ok(true) => match recipe_queue::start(project, task) {
            Ok(worker) => {
                notices.push(format!(
                    "Approved runtime recipe for task {} dispatched to the isolated task workspace.",
                    task.task_uid
                ));
                project.activity.runtime_recipe_worker = Some(worker);
                return true;
            }
            Err(reason) => notices.push(format!(
                "Approved runtime recipe for task {} could not start: {reason}",
                task.task_uid
            )),
        },
        Ok(false) => {}
        Err(reason) => {
            if qa_state.and_then(|state| state.blocked_reason.as_deref()) == Some(reason.as_str()) {
                return false;
            }
            let id = event_uid(
                &task.task_uid,
                task.revision,
                "runtime-recipe-attempt-blocked-v1",
            );
            match LifecycleStore::new(&project.state.planning_store).block_runtime_qa_unavailable(
                &task.task_uid,
                task.revision,
                &id,
                reason.clone(),
            ) {
                Ok(_) => notices.push(format!(
                    "Runtime QA for task {} is blocked by its runtime recipe attempt: {reason}",
                    task.task_uid
                )),
                Err(error) => project.activity.pending.push(format!(
                    "Runtime recipe blocker for task {} could not be persisted: {error}",
                    task.task_uid
                )),
            }
            return false;
        }
    }

    if request::selected_headless_target(project, task) {
        notices.push(format!(
            "Task {} targets a headless service. Visual QA is not a pass or an inferred skip; review and record applicability in Task Details.",
            task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
        ));
        return false;
    }
    match request::prepare(project, task) {
        Ok(prepared) => {
            let id = event_uid(&task.task_uid, task.revision, "runtime-qa-request-v1");
            match LifecycleStore::new(&project.state.planning_store).queue_runtime_qa(
                &task.task_uid,
                task.revision,
                &id,
                prepared.request.clone(),
            ) {
                Ok(_) => {
                    if qa_state.is_some_and(|state| {
                        state.status == crate::core::lifecycle::QaExecutionStatusV1::Blocked
                    }) {
                        notices.push(format!(
                            "Runtime QA for task {} has a fresh request persisted; dispatch is eligible on the next tick.",
                            task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                        ));
                    } else {
                        *launch = Some((prepared, task.task_uid.clone()));
                    }
                }
                Err(error) => project.activity.pending.push(format!(
                    "Runtime QA request for task {} could not be persisted: {error}",
                    task.task_uid
                )),
            }
        }
        Err(reason) => {
            if qa_state.and_then(|state| state.blocked_reason.as_deref()) == Some(reason.as_str()) {
                return false;
            }
            let id = event_uid(&task.task_uid, task.revision, "runtime-qa-unavailable-v1");
            match LifecycleStore::new(&project.state.planning_store)
                .block_runtime_qa_unavailable(
                    &task.task_uid,
                    task.revision,
                    &id,
                    reason.clone(),
                )
            {
                Ok(_) => notices.push(format!(
                    "Runtime QA for task {} is blocked before request creation: {reason} No runtime execution or result was produced.",
                    task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                )),
                Err(error) => project.activity.pending.push(format!(
                    "Runtime QA for task {} could not persist its request blocker: {error}",
                    task.task_uid
                )),
            }
        }
    }
    false
}
