use crate::{
    app::session::Project,
    core::{
        implementation::{self, ImplementationStatus, ManagerRepairContextV1},
        lifecycle::{LifecycleStore, TaskPhase},
        planning_work::{WorkKind, WorkStatus},
    },
    runtime_qa::{QaFindingDispositionKind, QaFindingDispositionStatus, RuntimeQaStore},
};

#[path = "qa_findings/finish.rs"]
mod finish;
pub(crate) use finish::finish_after_qa;

/// Dispatch one persisted blocking QA finding into the task's existing isolated clone.
/// The report and disposition are reloaded from the workspace QA store; board text is never
/// treated as authority for the code head or repair scope.
pub(crate) fn next_repair(project: &mut Project) -> Option<(String, String)> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || !project.active_implementations.is_empty()
    {
        return None;
    }
    let qa_root =
        RuntimeQaStore::local_root_for_workspace(project.state.planning_store.project_id).ok()?;
    next_repair_with_store(project, &RuntimeQaStore::new(qa_root))
}

#[cfg(test)]
pub(crate) fn next_repair_with_store_for_test(
    project: &mut Project,
    qa: &RuntimeQaStore,
) -> Option<(String, String)> {
    next_repair_with_store(project, qa)
}

fn next_repair_with_store(project: &mut Project, qa: &RuntimeQaStore) -> Option<(String, String)> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || !project.active_implementations.is_empty()
    {
        return None;
    }
    let work_index = project.planning_work.iter().position(|work| {
        work.kind == WorkKind::QaFindingDisposition && work.status == WorkStatus::Todo
    })?;
    let work = project.planning_work[work_index].clone();
    let task_uid = work.parent_uid.as_deref()?;
    let state = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))?
        .clone();
    if state.pr_url.is_some() || state.merged_commit.is_some() {
        return mark_attention(
            project,
            work_index,
            "The task is already published; Runtime QA repair cannot modify published history.",
        );
    }
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle.load_task(task_uid).ok()?;
    if task.repair_budget_exhausted() {
        return mark_attention(
            project,
            work_index,
            "The task exhausted its bounded repair attempts; Manager must review the finding and choose the next step.",
        );
    }
    let Some(expected_head) = task.reviewed_code_head_sha.as_deref() else {
        return mark_attention(
            project,
            work_index,
            "The task has no reviewed code head for the failed Runtime QA report.",
        );
    };
    if task.phase != TaskPhase::Repairing || state.verified_head.as_deref() != Some(expected_head) {
        return mark_attention(
            project,
            work_index,
            "The Runtime QA finding is stale for the current task head; Manager must review the latest evidence.",
        );
    }
    let Some((qa_run_id, finding_id)) = parse_work_key(&work.key) else {
        return mark_attention(
            project,
            work_index,
            "The Runtime QA repair work identity is malformed.",
        );
    };
    let (request, _, evidence, result) = match qa.load_current_run(qa_run_id) {
        Ok(report) => report,
        Err(_) => {
            return mark_attention(
                project,
                work_index,
                "The persisted Runtime QA report is unavailable or invalid; no repair was dispatched.",
            );
        }
    };
    if qa.load_gate_receipt(&request.qa_run_id).is_err() {
        return mark_attention(
            project,
            work_index,
            "The failed Runtime QA run has no valid independent-review receipt.",
        );
    }
    let Some(finding) = result.findings.iter().find(|finding| {
        finding.finding_uid == finding_id && finding.blocking && finding.code_sha == expected_head
    }) else {
        return mark_attention(
            project,
            work_index,
            "The failed Runtime QA run no longer contains this blocking finding.",
        );
    };
    let dispositions = match qa.load_dispositions(&request.qa_run_id) {
        Ok(dispositions) => dispositions,
        Err(_) => {
            return mark_attention(
                project,
                work_index,
                "The failed Runtime QA run has no valid persisted disposition.",
            );
        }
    };
    if request.parent_task_uid != task_uid
        || request.code_head_sha != expected_head
        || evidence.code_sha != expected_head
        || result.outcome != crate::runtime_qa::QaOutcome::Fail
        || !dispositions.iter().any(|disposition| {
            disposition.finding_id == finding_id
                && disposition.kind == QaFindingDispositionKind::RepairInScope
                && disposition.status == QaFindingDispositionStatus::Assigned
                && disposition.parent_task_id == task_uid
                && disposition.code_sha == expected_head
        })
    {
        return mark_attention(
            project,
            work_index,
            "The Runtime QA finding no longer matches its assigned failed run; no repair was dispatched.",
        );
    }
    let finding_record = serde_json::to_string(&serde_json::json!({
        "finding_uid": finding.finding_uid,
        "code_sha": finding.code_sha,
        "scenario_uid": finding.scenario_uid,
        "requirement_uids": finding.requirement_uids.iter().take(32).collect::<Vec<_>>(),
        "expected": crate::core::context_build::clip(&finding.expected, 1024),
        "actual": crate::core::context_build::clip(&finding.actual, 1024),
        "smallest_fix": crate::core::context_build::clip(&finding.smallest_fix, 1024),
        "reproducible_steps": finding.reproducible_steps.iter().take(12)
            .map(|step| crate::core::context_build::clip(step, 256)).collect::<Vec<_>>(),
        "screenshot_ids": finding.screenshot_ids.iter().take(12).collect::<Vec<_>>(),
        "trace_ids": finding.trace_ids.iter().take(24).collect::<Vec<_>>(),
    }))
    .ok()?;
    let context = ManagerRepairContextV1 {
        work_id: work.uid.clone(),
        task_uid: task_uid.to_owned(),
        finding_id: finding_id.to_owned(),
        finding_fingerprint: None,
        reviewed_subject_hash: match task.reviewed_code_subject_sha256.as_deref() {
            Some(hash) => hash.to_owned(),
            None => {
                return mark_attention(
                    project,
                    work_index,
                    "The task has no reviewed subject hash for the failed Runtime QA report.",
                );
            }
        },
        reviewed_head_sha: expected_head.to_owned(),
        evidence_ref: format!("runtime-qa-finding:{finding_id}"),
        manager_reason: crate::core::context_build::clip(&work.request, 2048),
        finding_record,
    };
    if context.validate().is_err() {
        return mark_attention(
            project,
            work_index,
            "The application-built Runtime QA repair context failed validation.",
        );
    }
    let ticket = state.ticket.clone();
    let mut updated = state.clone();
    updated.status = ImplementationStatus::ChangesRequested;
    updated.detail = format!(
        "Manager assigned scoped repair {} for blocking Runtime QA finding {}. Fresh independent code review and Runtime QA are required before reconciliation.",
        context.work_id, context.finding_id
    );
    updated.manager_repair_context = Some(context);
    let state_dir =
        match implementation::state_dir_for_task(&project.state.repo_root, &ticket, Some(task_uid))
        {
            Ok(path) => path,
            Err(_) => {
                return mark_attention(
                    project,
                    work_index,
                    "The isolated task state for the Runtime QA repair could not be located.",
                );
            }
        };
    if implementation::save(&state_dir, &updated).is_err() {
        return mark_attention(
            project,
            work_index,
            "The scoped Runtime QA repair assignment could not be persisted.",
        );
    }
    project
        .implementation_states
        .insert(ticket.clone(), updated);
    project.planning_work[work_index].status = WorkStatus::InProgress;
    project.planning_work[work_index].detail.push_str(
        "\n\nThe application dispatched this exact blocking QA finding to the existing isolated task clone. Fresh independent review and Runtime QA must pass on the changed head before reconciliation.",
    );
    if project.save_planning_work().is_err() {
        project.planning_work[work_index] = work;
        let _ = implementation::save(&state_dir, &state);
        project.implementation_states.insert(ticket.clone(), state);
        return None;
    }
    project.activity.pending.push(format!(
        "Manager dispatched a scoped repair for Runtime QA finding {finding_id} on task {task_uid}."
    ));
    Some((ticket, work.uid))
}

fn parse_work_key(key: &str) -> Option<(&str, &str)> {
    let mut parts = key.split(':');
    if parts.next() != Some("task") || parts.next() != Some("runtime-qa") {
        return None;
    }
    let run = parts.next()?;
    let finding = parts.next()?;
    if uuid::Uuid::parse_str(run).is_err()
        || uuid::Uuid::parse_str(finding).is_err()
        || parts.next().is_some()
    {
        return None;
    }
    Some((run, finding))
}

fn mark_attention(project: &mut Project, index: usize, detail: &str) -> Option<(String, String)> {
    project.planning_work[index].status = WorkStatus::NeedsAttention;
    project.planning_work[index]
        .detail
        .push_str(&format!("\n\nBlocked: {detail}"));
    if let Err(error) = project.save_planning_work() {
        project.activity.pending.push(format!(
            "Runtime QA repair requires attention, but its disposition could not be saved: {error}"
        ));
    }
    None
}

#[cfg(test)]
#[path = "qa_findings/tests.rs"]
mod tests;
