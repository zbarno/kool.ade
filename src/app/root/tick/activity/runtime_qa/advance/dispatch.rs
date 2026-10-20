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
        let store = crate::runtime_qa::RuntimeQaStore::new(
            project.state.planning_store.root.join("state/runtime-qa"),
        );
        match store.load_approved_scenarios(&prepared.request) {
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
