use super::super::{request::PreparedRequest, worker};
use crate::app::session::Project;

fn start_worker_or_block(
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
    task_revision: Result<u64, String>,
    qa_run_id: &str,
    start: impl FnOnce() -> Result<crate::runtime_qa::QaWorkerHandle, String>,
) -> Result<crate::runtime_qa::QaWorkerHandle, (String, Option<String>)> {
    match start() {
        Ok(worker) => Ok(worker),
        Err(reason) => {
            let blocked = task_revision.and_then(|revision| {
                let namespace = uuid::Uuid::parse_str(task_uid)
                    .map_err(|_| "runtime QA task identity is invalid".to_owned())?;
                let event_uid = uuid::Uuid::new_v5(
                    &namespace,
                    format!("runtime-qa-dispatch-failed-v1:{qa_run_id}").as_bytes(),
                )
                .hyphenated()
                .to_string();
                lifecycle
                    .block_runtime_qa(task_uid, revision, &event_uid, qa_run_id, reason.clone())
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
            Err((reason, blocked.err()))
        }
    }
}

pub(super) fn dispatch_queued_worker(
    project: &mut Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
    launch: Option<(PreparedRequest, String)>,
    notices: &mut Vec<String>,
) {
    if project.activity.runtime_qa_worker.is_none()
        && let Some((prepared, task_uid)) = launch
    {
        let harness = crate::app::root::configured_harness_for_project_task(
            task_harness,
            Some(crate::persistence::harness_settings::RUNTIME_QA),
            &project.state.harness_routes.routes,
            &prepared.task_routes,
        );
        let store = match crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        ) {
            Ok(root) => crate::runtime_qa::RuntimeQaStore::new(root),
            Err(error) => {
                notices.push(format!(
                    "Runtime QA local evidence storage is unavailable: {error}"
                ));
                return;
            }
        };
        let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
        let current_task = lifecycle
            .load_task(&task_uid)
            .map_err(|error| error.to_string());
        let approved = current_task.and_then(|task| {
            let proposal = store.load_scenario_proposal(&prepared.request)?;
            let evidence =
                super::super::request::scenario_synthesis_evidence(project, &task, &prepared)?;
            proposal.validate_source_evidence(&evidence)?;
            store.load_approved_scenario_proposal(&prepared.request, task.revision)
        });
        match approved {
            Ok(approved) => {
                let qa_run_id = prepared.request.qa_run_id.clone();
                let revision = lifecycle
                    .load_task(&task_uid)
                    .map(|task| task.revision)
                    .map_err(|error| error.to_string());
                match start_worker_or_block(
                    &lifecycle,
                    &task_uid,
                    revision,
                    &qa_run_id,
                    || worker::start(project, prepared, approved.scenarios, harness),
                ) {
                    Ok(worker) => {
                        notices.push(format!("Runtime QA for task {task_uid} dispatched to the selected independent reviewer and isolated Docker session."));
                        project.activity.runtime_qa_worker = Some(worker);
                    }
                    Err((reason, None)) => notices.push(format!(
                        "Runtime QA for task {task_uid} is durably blocked because its worker could not start: {reason}"
                    )),
                    Err((reason, Some(error))) => notices.push(format!(
                        "Runtime QA for task {task_uid} could not start ({reason}) and its blocker could not be persisted ({error})"
                    )),
                }
            }
            Err(error) => notices.push(format!(
                "Runtime QA scenario approval could not be reloaded: {error}"
            )),
        }
    }
}

#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;

pub(super) fn flush_notices(project: &mut Project, notices: Vec<String>) {
    for notice in notices {
        project.activity.pending.push(notice.clone());
        project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
            crate::domain::chatlog::ChatRole::System,
            notice,
            None,
        )]);
    }
}
