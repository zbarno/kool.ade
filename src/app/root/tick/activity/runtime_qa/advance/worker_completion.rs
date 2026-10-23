use crate::app::session::Project;

pub(super) struct CompletedWorkerState {
    pub identity: Option<(String, String)>,
    pub result: Option<Result<(), String>>,
}

pub(super) fn completed_worker_state(project: &mut Project) -> CompletedWorkerState {
    let progress_updates = project.activity.runtime_qa_worker.as_ref().map(|worker| {
        let mut updates = Vec::new();
        while let Some(update) = worker.take_progress() {
            updates.push(update);
        }
        (worker.task_uid.clone(), updates)
    });
    if let Some((task_uid, updates)) = progress_updates {
        for update in updates {
            let requests = update.dependency_requests.clone();
            let new_requests = {
                let activity = project.activity.tasks.entry(task_uid.clone()).or_default();
                let new_requests = requests
                    .into_iter()
                    .filter(|request| {
                        !activity
                            .dependency_requests
                            .iter()
                            .any(|known| known.id == request.id)
                    })
                    .collect::<Vec<_>>();
                activity.update(update.clone());
                new_requests
            };
            for request in new_requests {
                if request.status != crate::harness::DependencyRequestStatus::Failed {
                    project
                        .activity
                        .pending_dependency_reviews
                        .push((task_uid.clone(), request));
                }
            }
            project.live_progress.update(update);
            project.save_task_activity(&task_uid);
            project.activity.mark_ticket_dirty(&task_uid);
        }
    }
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
