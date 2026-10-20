use super::super::{request::PreparedRequest, worker};
use crate::app::session::Project;

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
        let current_revision = lifecycle
            .load_task(&task_uid)
            .map(|task| task.revision)
            .map_err(|error| error.to_string());
        match current_revision
            .and_then(|revision| store.load_approved_scenario_proposal(&prepared.request, revision))
        {
            Ok(approved) => match worker::start(project, prepared, approved.scenarios, harness) {
                Ok(worker) => {
                    notices.push(format!("Runtime QA for task {task_uid} dispatched to the selected independent reviewer and isolated Docker session."));
                    project.activity.runtime_qa_worker = Some(worker);
                }
                Err(reason) => notices.push(format!(
                    "Runtime QA for task {task_uid} could not start: {reason}"
                )),
            },
            Err(error) => notices.push(format!(
                "Runtime QA scenario approval could not be reloaded: {error}"
            )),
        }
    }
}

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
