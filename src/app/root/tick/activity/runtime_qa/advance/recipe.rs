use crate::app::session::Project;

pub(super) fn record_recipe_worker_completion(project: &mut Project) {
    let progress_updates = project
        .activity
        .runtime_recipe_worker
        .as_ref()
        .map(|worker| {
            let mut updates = Vec::new();
            while let Some(update) = worker.take_progress() {
                updates.push(update);
            }
            (worker.ticket.clone(), updates)
        });
    if let Some((ticket, updates)) = progress_updates {
        for update in updates {
            let requests = update.dependency_requests.clone();
            let new_requests = {
                let activity = project.activity.tasks.entry(ticket.clone()).or_default();
                let new_requests = requests
                    .into_iter()
                    .filter(|request| {
                        !activity
                            .dependency_requests
                            .iter()
                            .any(|known| known.id == request.id)
                    })
                    .collect::<Vec<_>>();
                activity.update(update);
                new_requests
            };
            for request in new_requests {
                if request.status != crate::harness::DependencyRequestStatus::Failed {
                    project
                        .activity
                        .pending_dependency_reviews
                        .push((ticket.clone(), request));
                }
            }
            project.save_task_activity(&ticket);
            project.activity.mark_ticket_dirty(&ticket);
        }
    }
    let recipe_worker_task = project
        .activity
        .runtime_recipe_worker
        .as_ref()
        .map(|worker| worker.task_uid.clone());
    let recipe_completion = project
        .activity
        .runtime_recipe_worker
        .as_ref()
        .map(crate::runtime_qa::RuntimeRecipeWorkerHandle::take_result);
    if recipe_completion
        .as_ref()
        .is_some_and(|result| result.is_err() || result.as_ref().is_ok_and(Option::is_some))
    {
        project.activity.runtime_recipe_worker = None;
    }
    if let Some(completion) = recipe_completion
        && let Ok(Some(result)) = completion
    {
        match result {
            Ok(receipts) => project.activity.pending.push(format!(
                "Approved runtime recipe queue completed {} operation receipt(s) for task {}.",
                receipts.len(),
                recipe_worker_task.as_deref().unwrap_or("unknown")
            )),
            Err(reason) => project.activity.pending.push(format!(
                "Approved runtime recipe queue for task {} stopped: {reason}",
                recipe_worker_task.as_deref().unwrap_or("unknown")
            )),
        }
    }
}
