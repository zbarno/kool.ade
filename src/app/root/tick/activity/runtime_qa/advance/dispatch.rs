use super::super::{request::PreparedRequest, worker};
use crate::app::session::Project;

fn matches_pending_launch(
    task: &crate::core::lifecycle::TaskRecord,
    expected_revision: u64,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> bool {
    task.revision == expected_revision
        && task.phase == crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
        && task.qa_execution.as_ref().is_some_and(|execution| {
            matches!(
                execution.status,
                crate::core::lifecycle::QaExecutionStatusV1::Pending
                    | crate::core::lifecycle::QaExecutionStatusV1::Running
            ) && execution.request.as_ref() == Some(request)
        })
}

enum DispatchStartError {
    ActiveWorker,
    ReportPresent,
    Blocked {
        reason: String,
        persistence_error: Option<String>,
    },
}

fn start_worker_or_block(
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
    qa_run_id: &str,
    lock_path: &std::path::Path,
    start: impl FnOnce() -> Result<crate::runtime_qa::QaWorkerHandle, worker::StartError>,
) -> Result<crate::runtime_qa::QaWorkerHandle, DispatchStartError> {
    match start() {
        Ok(worker) => Ok(worker),
        Err(worker::StartError::ActiveWorker) => Err(DispatchStartError::ActiveWorker),
        Err(worker::StartError::ReportPresent) => Err(DispatchStartError::ReportPresent),
        Err(worker::StartError::Failed(reason)) => {
            let lock = match worker::acquire_run_lock(lock_path) {
                Ok(lock) => lock,
                Err(worker::RunLockError::ActiveWorker) => {
                    return Err(DispatchStartError::ActiveWorker);
                }
                Err(worker::RunLockError::Failed(error)) => {
                    return Err(DispatchStartError::Blocked {
                        reason,
                        persistence_error: Some(format!(
                            "could not acquire the run lock to record the dispatch failure: {error}"
                        )),
                    });
                }
            };
            let blocked = lifecycle
                .load_task(task_uid)
                .map_err(|error| error.to_string())
                .and_then(|task| {
                    if task.qa_execution.as_ref().is_none_or(|execution| {
                        !matches!(
                            execution.status,
                            crate::core::lifecycle::QaExecutionStatusV1::Pending
                                | crate::core::lifecycle::QaExecutionStatusV1::Running
                        ) || execution
                            .request
                            .as_ref()
                            .is_none_or(|request| request.qa_run_id != qa_run_id)
                    }) {
                        return Err(
                            "queued request changed before dispatch failure was recorded".into(),
                        );
                    }
                    let namespace = uuid::Uuid::parse_str(task_uid)
                        .map_err(|_| "runtime QA task identity is invalid".to_owned())?;
                    let event_uid = uuid::Uuid::new_v5(
                        &namespace,
                        format!("runtime-qa-dispatch-failed-v1:{qa_run_id}").as_bytes(),
                    )
                    .hyphenated()
                    .to_string();
                    lifecycle
                        .block_runtime_qa(
                            task_uid,
                            task.revision,
                            &event_uid,
                            qa_run_id,
                            reason.clone(),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                });
            drop(lock);
            Err(DispatchStartError::Blocked {
                reason,
                persistence_error: blocked.err(),
            })
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
        let store_root = match crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        ) {
            Ok(root) => root,
            Err(error) => {
                notices.push(format!(
                    "Runtime QA local evidence storage is unavailable: {error}"
                ));
                return;
            }
        };
        let store = crate::runtime_qa::RuntimeQaStore::new(store_root.clone());
        let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
        let current_task = lifecycle
            .load_task(&task_uid)
            .map_err(|error| error.to_string());
        let approved = current_task.and_then(|task| {
            let proposal = store.load_scenario_proposal(&prepared.request)?;
            let evidence =
                super::super::request::scenario_synthesis_evidence(project, &task, &prepared)?;
            proposal.validate_source_evidence(&evidence)?;
            let approval_revision = task
                .qa_execution
                .as_ref()
                .and_then(|execution| execution.dispatch_task_revision)
                .unwrap_or(task.revision);
            let approved =
                store.load_approved_scenario_proposal(&prepared.request, approval_revision)?;
            Ok((task.revision, approved))
        });
        match approved {
            Ok((validated_revision, approved)) => {
                let qa_run_id = prepared.request.qa_run_id.clone();
                let lock_path = store_root.join(&qa_run_id).join("worker.lock");
                let current = lifecycle.load_task(&task_uid);
                if !current.as_ref().is_ok_and(|task| {
                    matches_pending_launch(task, validated_revision, &prepared.request)
                }) {
                    notices.push(format!(
                        "Runtime QA request for task {task_uid} changed before dispatch; the stale approval was not started."
                    ));
                    return;
                }
                match start_worker_or_block(
                    &lifecycle,
                    &task_uid,
                    &qa_run_id,
                    &lock_path,
                    || {
                        worker::start(
                            project,
                            prepared,
                            approved.scenarios,
                            harness,
                            validated_revision,
                        )
                    },
                ) {
                    Ok(worker) => {
                        notices.push(format!("Runtime QA for task {task_uid} dispatched to the selected independent reviewer and isolated Docker session."));
                        project.activity.runtime_qa_worker = Some(worker);
                    }
                    Err(DispatchStartError::ActiveWorker) => {}
                    Err(DispatchStartError::ReportPresent) => notices.push(format!(
                        "A completed Runtime QA report for task {task_uid} will be applied instead of restarting its worker."
                    )),
                    Err(DispatchStartError::Blocked {
                        reason,
                        persistence_error: None,
                    }) => notices.push(format!(
                        "Runtime QA for task {task_uid} is durably blocked because its worker could not start: {reason}"
                    )),
                    Err(DispatchStartError::Blocked {
                        reason,
                        persistence_error: Some(error),
                    }) => notices.push(format!(
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
