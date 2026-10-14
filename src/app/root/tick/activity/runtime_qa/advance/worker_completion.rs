use crate::app::session::Project;

pub(super) struct CompletedWorkerState {
    pub identity: Option<(String, String)>,
    pub result: Option<Result<(), String>>,
}

pub(super) fn completed_worker_state(project: &mut Project) -> CompletedWorkerState {
    let completed_worker_identity = project
        .activity
        .runtime_qa_worker
        .as_ref()
        .map(|worker| (worker.qa_run_id.clone(), worker.task_uid.clone()));
    let completed_worker = project
        .activity
        .runtime_qa_worker
        .as_ref()
        .map(crate::runtime_qa::QaWorkerHandle::take_result);
    if completed_worker.as_ref().is_some_and(|completion| {
        completion.is_err() || completion.as_ref().is_ok_and(Option::is_some)
    }) {
        project.activity.runtime_qa_worker = None;
    }
    let worker_result = completed_worker
        .as_ref()
        .map(|completion| match completion {
            Ok(Some(result)) => result.clone(),
            Err(()) => Err("Runtime QA worker exited without returning a result".into()),
            Ok(None) => Ok(()),
        })
        .filter(|_| {
            completed_worker.as_ref().is_some_and(|completion| {
                completion.is_err() || completion.as_ref().is_ok_and(Option::is_some)
            })
        });
    CompletedWorkerState {
        identity: completed_worker_identity,
        result: worker_result,
    }
}
