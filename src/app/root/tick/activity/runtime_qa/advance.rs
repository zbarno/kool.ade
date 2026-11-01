use super::*;

mod blocking;
mod dispatch;
mod queue;
mod recipe;
mod record_report;
mod worker_completion;

pub(in crate::app::root::tick::activity) fn advance_runtime_qa(
    project: &mut Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    let applicability_result = project
        .activity
        .runtime_qa_applicability_review
        .as_ref()
        .and_then(|worker| worker.take_result());
    if let Some(result) = applicability_result {
        project.activity.runtime_qa_applicability_review = None;
        let notice = match result {
            Ok(proposal) => match crate::app::root::runtime_qa_applicability::persist_review(
                project, &proposal,
            ) {
                Ok(()) => "Independent headless applicability review is ready in Task Details for Manager approval.".into(),
                Err(error) => format!("Headless applicability review was discarded because its task or target changed: {error}"),
            },
            Err(error) => format!("Headless applicability review did not produce an approved proposal: {error}"),
        };
        project.activity.pending.push(notice);
    }
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
            if queue::process_without_request(project, &task, qa_state, &mut notices, &mut launch) {
                break;
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
        let qa_root = match crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        ) {
            Ok(root) => root,
            Err(error) => {
                notices.push(format!(
                    "Runtime QA local evidence storage is unavailable: {error}"
                ));
                continue;
            }
        };
        let qa_store = crate::runtime_qa::RuntimeQaStore::new(qa_root.clone());
        if completed_worker_identity
            .as_ref()
            .is_some_and(|(run_id, uid)| run_id == &request.qa_run_id && uid == &task.task_uid)
            && let Some(Err(reason)) = worker_result.as_ref()
            && project.activity.runtime_qa_worker.is_none()
        {
            match blocking::persist_block_with_work(
                &project.state.planning_store,
                blocking::PlanningWorkState {
                    items: &mut project.planning_work,
                    baseline_revision: &mut project.state.baseline_planning_revision,
                },
                &qa_root,
                &task,
                request,
                "runtime-qa-worker-failed-v1",
                reason.clone(),
            ) {
                Ok(blocking::BlockOutcome::Persisted) => notices.push(format!(
                    "Runtime QA for task {} is blocked: {reason}",
                    task.task_uid
                )),
                Ok(blocking::BlockOutcome::ActiveWorker) => notices.push(format!(
                    "Runtime QA for task {} is still owned by an active worker; its state was left unchanged.",
                    task.task_uid
                )),
                Ok(blocking::BlockOutcome::Stale) => notices.push(format!(
                    "Runtime QA for task {} changed before its worker failure could be recorded.",
                    task.task_uid
                )),
                Ok(blocking::BlockOutcome::ReportPresent { notice, applied }) => {
                    if applied {
                        record_report::finish_present(project, &task, request, &qa_store);
                    }
                    notices.push(notice);
                }
                Err(error) => project.activity.pending.push(format!(
                    "Runtime QA worker failure could not be persisted: {error}"
                )),
            }
            continue;
        }
        match record_report::record(project, &task, request, &qa_store) {
            Ok(Some(notice)) => {
                notices.push(notice);
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                let reason = format!(
                    "a persisted QA report could not be safely accepted ({error}); inspect the exact run evidence and create a fresh reviewed request before retrying."
                );
                match blocking::persist_block_with_work(
                    &project.state.planning_store,
                    blocking::PlanningWorkState {
                        items: &mut project.planning_work,
                        baseline_revision: &mut project.state.baseline_planning_revision,
                    },
                    &qa_root,
                    &task,
                    request,
                    "runtime-qa-report-rejected-v1",
                    reason.clone(),
                ) {
                    Ok(blocking::BlockOutcome::Persisted) => notices.push(format!(
                        "Runtime QA report for task {} was rejected and blocked: {reason}",
                        task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                    )),
                    Ok(blocking::BlockOutcome::ActiveWorker) => notices.push(format!(
                        "Runtime QA report for task {} is still being written by an active worker; its state was left unchanged.",
                        task.task_uid
                    )),
                    Ok(blocking::BlockOutcome::Stale) => notices.push(format!(
                        "Runtime QA report for task {} changed before the report could be rejected.",
                        task.task_uid
                    )),
                    Ok(blocking::BlockOutcome::ReportPresent { notice, applied }) => {
                        if applied {
                            record_report::finish_present(project, &task, request, &qa_store);
                        }
                        notices.push(notice);
                    }
                    Err(block_error) => project.activity.pending.push(format!(
                        "Runtime QA report for task {} could not be recorded or blocked: {block_error}",
                        task.task_uid
                    )),
                }
                continue;
            }
        }
        let approval_revision = qa_state
            .and_then(|state| state.dispatch_task_revision)
            .unwrap_or(task.revision);
        if let Err(error) = qa_store.load_approved_scenario_proposal(request, approval_revision) {
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
                match blocking::persist_block_with_work(&project.state.planning_store, blocking::PlanningWorkState { items: &mut project.planning_work, baseline_revision: &mut project.state.baseline_planning_revision }, &qa_root, &task, request, "runtime-qa-request-stale-v1", reason.into()) {
                    Ok(blocking::BlockOutcome::Persisted) => notices.push(format!("Runtime QA for task {} was blocked because its queued request is stale; issue a fresh reviewed request.", task.task_uid)),
                    Ok(blocking::BlockOutcome::ActiveWorker) => notices.push(format!("Runtime QA for task {} is still owned by an active worker; its state was left unchanged.", task.task_uid)),
                    Ok(blocking::BlockOutcome::Stale) => notices.push(format!("Runtime QA for task {} changed before its stale request could be blocked.", task.task_uid)),
                    Ok(blocking::BlockOutcome::ReportPresent { notice, applied }) => {
                        if applied {
                            record_report::finish_present(project, &task, request, &qa_store);
                        }
                        notices.push(notice);
                    }
                    Err(error) => project.activity.pending.push(format!("Stale Runtime QA request could not be blocked: {error}")),
                }
                continue;
            }
            Err(reason) => {
                let reason = format!(
                    "queued Runtime QA request inputs cannot be reconstructed: {reason}; issue a fresh reviewed request."
                );
                match blocking::persist_block_with_work(
                    &project.state.planning_store,
                    blocking::PlanningWorkState {
                        items: &mut project.planning_work,
                        baseline_revision: &mut project.state.baseline_planning_revision,
                    },
                    &qa_root,
                    &task,
                    request,
                    "runtime-qa-inputs-missing-v1",
                    reason.clone(),
                ) {
                    Ok(blocking::BlockOutcome::Persisted) => notices.push(format!(
                        "Runtime QA for task {} was blocked: {reason}",
                        task.task_uid
                    )),
                    Ok(blocking::BlockOutcome::ActiveWorker) => notices.push(format!(
                        "Runtime QA for task {} is still owned by an active worker; its state was left unchanged.",
                        task.task_uid
                    )),
                    Ok(blocking::BlockOutcome::Stale) => notices.push(format!(
                        "Runtime QA for task {} changed before its input blocker could be recorded.",
                        task.task_uid
                    )),
                    Ok(blocking::BlockOutcome::ReportPresent { notice, applied }) => {
                        if applied {
                            record_report::finish_present(project, &task, request, &qa_store);
                        }
                        notices.push(notice);
                    }
                    Err(error) => project.activity.pending.push(format!(
                        "Runtime QA input blocker could not be persisted: {error}"
                    )),
                }
                continue;
            }
        };
        if let Some(reason) = unavailable_reason(project, &task, request) {
            let blocked = match blocking::persist_block_with_work(
                &project.state.planning_store,
                blocking::PlanningWorkState {
                    items: &mut project.planning_work,
                    baseline_revision: &mut project.state.baseline_planning_revision,
                },
                &qa_root,
                &task,
                request,
                "runtime-qa-infrastructure-blocked-v1",
                reason.clone(),
            ) {
                Ok(blocking::BlockOutcome::Persisted) => {
                    notices.push(format!(
                        "Runtime QA for task {} is infrastructure-blocked: {reason}",
                        task.planning_item_uid.as_deref().unwrap_or(&task.task_uid)
                    ));
                    true
                }
                Ok(blocking::BlockOutcome::ActiveWorker) => {
                    notices.push(format!(
                        "Runtime QA for task {} is still owned by an active worker; its state was left unchanged.",
                        task.task_uid
                    ));
                    false
                }
                Ok(blocking::BlockOutcome::Stale) => {
                    notices.push(format!(
                        "Runtime QA for task {} changed before its infrastructure blocker could be recorded.",
                        task.task_uid
                    ));
                    false
                }
                Ok(blocking::BlockOutcome::ReportPresent { notice, applied }) => {
                    if applied {
                        record_report::finish_present(project, &task, request, &qa_store);
                    }
                    notices.push(notice);
                    false
                }
                Err(error) => {
                    project.activity.pending.push(format!(
                        "Runtime QA blocker could not be persisted: {error}"
                    ));
                    false
                }
            };
            if !blocked {
                continue;
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
