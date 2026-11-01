use super::parse_work_key;
use crate::{
    app::session::Project,
    core::{
        implementation,
        lifecycle::{GateDecision, LifecycleStore, QaExecutionStatusV1, TaskPhase},
        planning_work::{WorkKind, WorkStatus},
    },
    runtime_qa::{QaOutcome, QaResultV1, QaRunRequestV1, RuntimeQaStore},
};
use sha2::Digest as _;

pub(crate) fn finish_after_qa(
    project: &mut Project,
    task_uid: &str,
    request: &QaRunRequestV1,
    result: &QaResultV1,
) {
    if result.outcome != QaOutcome::Pass
        || request.parent_task_uid != task_uid
        || request.code_head_sha.is_empty()
    {
        return;
    }
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let Ok(task) = lifecycle.load_task(task_uid) else {
        return;
    };
    let Some(execution) = task.qa_execution.as_ref() else {
        return;
    };
    if task.phase != TaskPhase::AwaitingSpecReconciliation
        || execution.status != QaExecutionStatusV1::Completed
        || execution.request.as_ref() != Some(request)
    {
        return;
    }
    let Ok(root) =
        RuntimeQaStore::local_root_for_workspace(project.state.planning_store.project_id)
    else {
        return;
    };
    let store = RuntimeQaStore::new(root);
    let Ok((persisted_request, _, evidence, persisted_result)) =
        store.load_current_run(&request.qa_run_id)
    else {
        return;
    };
    let Ok(encoded_result) = serde_json::to_vec(&persisted_result) else {
        return;
    };
    let result_sha256 = format!("{:x}", sha2::Sha256::digest(encoded_result));
    let Ok(evidence_sha256) = evidence.sha256() else {
        return;
    };
    let Ok(receipt) = store.load_gate_receipt(&request.qa_run_id) else {
        return;
    };
    if persisted_request != *request
        || persisted_result != *result
        || execution.result_sha256.as_deref() != Some(result_sha256.as_str())
        || execution.evidence_manifest_sha256.as_deref() != Some(evidence_sha256.as_str())
        || execution.reviewer_execution_id.as_deref()
            != Some(persisted_result.reviewer_execution_id.as_str())
        || execution.reviewer_session_id.as_deref()
            != Some(persisted_result.reviewer_session_id.as_str())
        || receipt.decision != GateDecision::Approved
        || receipt.reviewer_run_id.as_deref()
            != Some(persisted_result.reviewer_execution_id.as_str())
        || receipt.reviewer_session_id.as_deref()
            != Some(persisted_result.reviewer_session_id.as_str())
    {
        return;
    }
    let resolved = project
        .planning_work
        .iter()
        .filter(|work| {
            work.kind == WorkKind::QaFindingDisposition
                && work.status != WorkStatus::Done
                && work.parent_uid.as_deref() == Some(task_uid)
        })
        .filter(|work| {
            let Some((prior_run_id, finding_id)) = parse_work_key(&work.key) else {
                return false;
            };
            let Ok((prior_request, _, _, prior_result)) = store.load_current_run(prior_run_id)
            else {
                return false;
            };
            prior_request.parent_task_uid == task_uid
                && prior_request.code_head_sha != request.code_head_sha
                && prior_result.outcome == QaOutcome::Fail
                && prior_result
                    .findings
                    .iter()
                    .any(|finding| finding.finding_uid == finding_id && finding.blocking)
        })
        .map(|work| work.uid.clone())
        .collect::<std::collections::HashSet<_>>();
    if resolved.is_empty() {
        return;
    }
    let original = project.planning_work.clone();
    for work in &mut project.planning_work {
        if resolved.contains(&work.uid) {
            work.status = WorkStatus::Done;
            work.detail.push_str(&format!(
                "\n\nResolved by passing Runtime QA on changed code head {} after fresh independent review.",
                request.code_head_sha
            ));
        }
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.activity.pending.push(format!(
            "Runtime QA passed for task {task_uid}, but its Manager repair disposition could not be closed: {error}"
        ));
        return;
    }
    if let Some((ticket, mut state)) = project
        .implementation_states
        .iter()
        .find(|(_, state)| state.task_uid.as_deref() == Some(task_uid))
        .map(|(ticket, state)| (ticket.clone(), state.clone()))
        && state
            .manager_repair_context
            .as_ref()
            .is_some_and(|context| resolved.contains(&context.work_id) && context.is_runtime_qa())
    {
        state.manager_repair_context = None;
        if let Ok(path) =
            implementation::state_dir_for_task(&project.state.repo_root, &ticket, Some(task_uid))
            && implementation::save(&path, &state).is_ok()
        {
            project.implementation_states.insert(ticket, state);
        }
    }
}
