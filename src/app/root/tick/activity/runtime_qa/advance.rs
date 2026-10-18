use super::*;

mod dispatch;
mod recipe;
mod worker_completion;

pub(in crate::app::root::tick::activity) fn advance_runtime_qa(
    project: &mut Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    let synthesis_result = project
        .activity
        .runtime_qa_scenario_synthesis
        .as_ref()
        .and_then(|worker| worker.take_result());
    if let Some(result) = synthesis_result {
        project.activity.runtime_qa_scenario_synthesis = None;
        let notice = match result {
            Ok((request, proposal)) => match super::persist_synthesized_proposal(project, &request, &proposal) {
                Ok(()) => "Runtime QA scenario proposal was saved. Review the exact scenarios in Task Details and approve them before execution.".into(),
                Err(error) => format!("Runtime QA scenario proposal was discarded because its request or task revision changed: {error}"),
            },
            Err(error) => format!("Runtime QA scenario synthesis failed: {error}"),
        };
        project.activity.pending.push(notice);
    }
    recipe::record_recipe_worker_completion(project);
    let completed_worker = worker_completion::completed_worker_state(project);
    let completed_worker_identity = completed_worker.identity;
    let worker_result = completed_worker.result;
    let candidates =
        match LifecycleStore::new(&project.state.planning_store).runtime_qa_candidates() {
            Ok(candidates) => candidates,
            Err(error) => {
                project
                    .activity
                    .pending
                    .push(format!("Runtime QA queue could not be read: {error}"));
                return;
            }
        };
    let mut notices = Vec::new();
    let mut launch = None;
    for task in candidates {
        if project.activity.runtime_recipe_worker.is_some() {
            break;
        }
        let qa_state = task.qa_execution.as_ref();
        let request = qa_state.and_then(|state| state.request.as_ref());
        if request.is_none() {
            match recipe_queue::ready(project, &task) {
                Ok(true) => match recipe_queue::start(project, &task) {
                    Ok(worker) => {
                        notices.push(format!(
                            "Approved runtime recipe for task {} dispatched to the isolated task workspace.",
                            task.task_uid
                        ));
                        project.activity.runtime_recipe_worker = Some(worker);
                        break;
                    }
                    Err(reason) => notices.push(format!(
                        "Approved runtime recipe for task {} could not start: {reason}",
                        task.task_uid
                    )),
                },
                Ok(false) => {}
                Err(reason) => {
                    if qa_state.and_then(|state| state.blocked_reason.as_deref())
                        == Some(reason.as_str())
                    {
                        continue;
                    }
                    let event_uid = event_uid(
                        &task.task_uid,
                        task.revision,
                        "runtime-recipe-attempt-blocked-v1",
                    );
                    match LifecycleStore::new(&project.state.planning_store)
                        .block_runtime_qa_unavailable(
                        &task.task_uid, task.revision, &event_uid, reason.clone(),
                    ) {
                        Ok(_) => notices.push(format!(
                            "Runtime QA for task {} is blocked by its runtime recipe attempt: {reason}",
                            task.task_uid
                        )),
                        Err(error) => project.activity.pending.push(format!(
                            "Runtime recipe blocker for task {} could not be persisted: {error}", task.task_uid
                        )),
                    }
                    continue;
                }
            }
            match request::prepare(project, &task) {
                Ok(prepared) => {
                    let event_uid =
                        event_uid(&task.task_uid, task.revision, "runtime-qa-request-v1");
                    match LifecycleStore::new(&project.state.planning_store).queue_runtime_qa(
                        &task.task_uid,
                        task.revision,
                        &event_uid,
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
                                launch = Some((prepared, task.task_uid.clone()));
                            }
                        }
                        Err(error) => project.activity.pending.push(format!(
                            "Runtime QA request for task {} could not be persisted: {error}",
                            task.task_uid
                        )),
                    }
                }
                Err(reason) => {
                    if qa_state.and_then(|state| state.blocked_reason.as_deref())
                        == Some(reason.as_str())
                    {
                        continue;
                    }
                    let event_uid =
                        event_uid(&task.task_uid, task.revision, "runtime-qa-unavailable-v1");
                    match LifecycleStore::new(&project.state.planning_store)
                        .block_runtime_qa_unavailable(
                        &task.task_uid,
                        task.revision,
                        &event_uid,
                        reason.clone(),
                    ) {
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
            continue;
        }
        let request = request.expect("checked above");
        if qa_state.is_some_and(|state| {
            state.status == crate::core::lifecycle::QaExecutionStatusV1::Blocked
        }) {
            // This exact run has already ended in a durable block. Retrying it
            // would reuse reviewer/session identity and evidence bindings.
            continue;
        }
        if project.activity.runtime_qa_worker.is_some() {
            continue;
        }
        let qa_store = match crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        ) {
            Ok(root) => crate::runtime_qa::RuntimeQaStore::new(root),
            Err(error) => {
                notices.push(format!(
                    "Runtime QA local evidence storage is unavailable: {error}"
                ));
                continue;
            }
        };
        if completed_worker_identity
            .as_ref()
            .is_some_and(|(run_id, uid)| run_id == &request.qa_run_id && uid == &task.task_uid)
            && let Some(Err(reason)) = worker_result.as_ref()
            && project.activity.runtime_qa_worker.is_none()
        {
            let event_uid = event_uid(&task.task_uid, task.revision, "runtime-qa-worker-failed-v1");
            match LifecycleStore::new(&project.state.planning_store).block_runtime_qa(
                &task.task_uid,
                task.revision,
                &event_uid,
                &request.qa_run_id,
                reason.clone(),
            ) {
                Ok(_) => notices.push(format!(
                    "Runtime QA for task {} is blocked: {reason}",
                    task.task_uid
                )),
                Err(error) => project.activity.pending.push(format!(
                    "Runtime QA worker failure could not be persisted: {error}"
                )),
            }
            continue;
        }
        match reports::record_persisted_report(
            &project.state.planning_store,
            &LifecycleStore::new(&project.state.planning_store),
            &qa_store,
            &task,
        ) {
            Ok(Some(notice)) => {
                notices.push(notice);
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                let reason = format!(
                    "a persisted QA report could not be safely accepted ({error}); inspect the exact run evidence and create a fresh reviewed request before retrying."
                );
                let event_uid = event_uid(
                    &task.task_uid,
                    task.revision,
                    "runtime-qa-report-rejected-v1",
                );
                match LifecycleStore::new(&project.state.planning_store).block_runtime_qa(
                    &task.task_uid,
                    task.revision,
                    &event_uid,
                    &request.qa_run_id,
                    reason.clone(),
                ) {
                    Ok(_) => notices.push(format!(
                        "Runtime QA report for task {} was rejected and blocked: {reason}",
                        task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                    )),
                    Err(block_error) => project.activity.pending.push(format!(
                        "Runtime QA report for task {} could not be recorded or blocked: {block_error}",
                        task.task_uid
                    )),
                }
                continue;
            }
        }
        if let Err(error) = qa_store.load_approved_scenario_proposal(request, task.revision) {
            notices.push(format!(
                "Runtime QA for task {} is waiting for explicit approval of its current synthesized scenario proposal ({error}). No scenarios have been executed.",
                task.planning_item_uid.as_deref().unwrap_or(task.task_uid.as_str())
            ));
            continue;
        }
        let prepared = match request::prepare(project, &task) {
            Ok(prepared) if prepared.request == *request => prepared,
            Ok(_) => {
                let reason = "queued Runtime QA request no longer matches the exact reviewed task, target, recipe, or build receipt; issue a fresh reviewed request.";
                let event_uid =
                    event_uid(&task.task_uid, task.revision, "runtime-qa-request-stale-v1");
                match LifecycleStore::new(&project.state.planning_store).block_runtime_qa(&task.task_uid, task.revision, &event_uid, &request.qa_run_id, reason.into()) {
                    Ok(_) => notices.push(format!("Runtime QA for task {} was blocked because its queued request is stale; issue a fresh reviewed request.", task.task_uid)),
                    Err(error) => project.activity.pending.push(format!("Stale Runtime QA request could not be blocked: {error}")),
                }
                continue;
            }
            Err(reason) => {
                let reason = format!(
                    "queued Runtime QA request inputs cannot be reconstructed: {reason}; issue a fresh reviewed request."
                );
                let event_uid = event_uid(
                    &task.task_uid,
                    task.revision,
                    "runtime-qa-inputs-missing-v1",
                );
                match LifecycleStore::new(&project.state.planning_store).block_runtime_qa(
                    &task.task_uid,
                    task.revision,
                    &event_uid,
                    &request.qa_run_id,
                    reason.clone(),
                ) {
                    Ok(_) => notices.push(format!(
                        "Runtime QA for task {} was blocked: {reason}",
                        task.task_uid
                    )),
                    Err(error) => project.activity.pending.push(format!(
                        "Runtime QA input blocker could not be persisted: {error}"
                    )),
                }
                continue;
            }
        };
        if let Some(reason) = unavailable_reason(project, &task, request) {
            let event_uid = event_uid(
                &task.task_uid,
                task.revision,
                "runtime-qa-infrastructure-blocked-v1",
            );
            match LifecycleStore::new(&project.state.planning_store).block_runtime_qa(
                &task.task_uid,
                task.revision,
                &event_uid,
                &request.qa_run_id,
                reason.clone(),
            ) {
                Ok(_) => notices.push(format!(
                    "Runtime QA for task {} is infrastructure-blocked: {reason}",
                    task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                )),
                Err(error) => project.activity.pending.push(format!(
                    "Runtime QA blocker could not be persisted: {error}"
                )),
            }
            let requested_toolchains =
                prerequisites::queue_missing_toolchain_reviews(project, &task, request);
            if !requested_toolchains.is_empty() {
                notices.push(format!(
                    "Man.ager review requests were queued for missing runtime toolchains: {}.",
                    requested_toolchains.join(", ")
                ));
            }
            continue;
        }
        launch = Some((prepared, task.task_uid.clone()));
        break;
    }
    dispatch::dispatch_queued_worker(project, task_harness, launch, &mut notices);
    dispatch::flush_notices(project, notices);
}
