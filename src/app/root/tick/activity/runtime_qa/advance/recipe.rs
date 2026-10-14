use crate::app::session::Project;

pub(super) fn record_recipe_worker_completion(project: &mut Project) {
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
