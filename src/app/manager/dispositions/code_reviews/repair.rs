use crate::{
    app::manager::catalog::code_reviews::current_findings,
    app::session::Project,
    core::{
        implementation::{self, ImplementationStatus, ManagerRepairContextV1},
        lifecycle::{LifecycleStore, TaskPhase},
        planning_work::{CodeReviewDispositionStatusV1, Work, WorkKind, WorkStatus},
    },
    harness::prompt_catalog::FindingScopeV1,
};
#[path = "repair/arbitration.rs"]
mod arbitration;
#[path = "repair/completion.rs"]
mod completion;
#[path = "repair/failure.rs"]
mod failure;

pub(crate) use arbitration::{CodeReviewArbitrationRequest, finish_arbitration, next_arbitration};
pub(crate) use completion::finish_repair;
pub(crate) use failure::mark_repair_failed;

pub(crate) fn next_repair(project: &mut Project) -> Option<(String, String)> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || !project.active_implementations.is_empty()
    {
        return None;
    }
    let work_index = project.planning_work.iter().position(|work| {
        work.kind == WorkKind::CodeReviewDisposition
            && work.status == WorkStatus::Todo
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| {
                    disposition.status == CodeReviewDispositionStatusV1::AssignedRepair
                })
    })?;
    let work = project.planning_work[work_index].clone();
    let disposition = work.code_review_disposition.clone()?;
    let task_uid = work.parent_uid.as_deref()?;
    let state = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))?
        .clone();
    if state.pr_url.is_some() || state.merged_commit.is_some() {
        return mark_dispatch_attention(
            project,
            work_index,
            "The reviewed task has already been published or merged; the scoped repair cannot modify its published history.",
        );
    }
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle.task_for_planning_item(task_uid).ok().flatten()?;
    if task.repair_budget_exhausted() {
        return mark_dispatch_attention(
            project,
            work_index,
            "The task exhausted its bounded repair attempts. Review the recurring finding and current diff, then re-plan the task or choose an explicit follow-up decision; no further repair was dispatched.",
        );
    }
    if task.phase != TaskPhase::Repairing
        || state.verified_head.as_deref() != Some(task.reviewed_code_head_sha.as_deref()?)
        || task.reviewed_code_subject_sha256.as_deref() != Some(disposition.subject_hash.as_str())
    {
        return mark_dispatch_attention(
            project,
            work_index,
            "The code-review disposition is stale for the current lifecycle subject; Manager must review the latest evidence.",
        );
    }
    let summary = match implementation::manager_findings_for_task(
        &project.state.repo_root,
        &state.ticket,
        task_uid,
    ) {
        Ok(Some(summary)) => summary,
        _ => {
            return mark_dispatch_attention(
                project,
                work_index,
                "The validated current code-review finding is unavailable; no repair was dispatched.",
            );
        }
    };
    if summary.pass
        || summary.subject_hash != disposition.subject_hash
        || summary.head_sha != state.verified_head.as_deref().unwrap_or_default()
        || !summary.findings.iter().any(|finding| {
            finding.id == disposition.finding_id && finding.scope == Some(FindingScopeV1::Blocking)
        })
    {
        return mark_dispatch_attention(
            project,
            work_index,
            "The assigned blocking finding no longer matches the reviewed receipt; Manager must re-evaluate the current subject.",
        );
    }
    let Some((finding, target)) =
        current_findings(project)
            .ok()
            .into_iter()
            .flatten()
            .find(|(item, current)| {
                item.id == disposition.evidence_ref
                    && current.task_uid == task_uid
                    && current.finding_id == disposition.finding_id
                    && current.subject_hash == disposition.subject_hash
                    && current.scope == FindingScopeV1::Blocking
            })
    else {
        return mark_dispatch_attention(
            project,
            work_index,
            "The assigned blocking finding no longer matches the reviewed receipt; Manager must re-evaluate the current subject.",
        );
    };
    let work_id = work.uid.clone();
    let context = ManagerRepairContextV1 {
        work_id: work_id.clone(),
        task_uid: task_uid.to_owned(),
        finding_id: disposition.finding_id,
        finding_fingerprint: disposition.finding_fingerprint,
        reviewed_subject_hash: disposition.subject_hash,
        reviewed_head_sha: summary.head_sha,
        evidence_ref: finding.id,
        manager_reason: disposition.decision_reason,
        finding_record: finding.content,
    };
    if context.validate().is_err() || target.scope != FindingScopeV1::Blocking {
        return mark_dispatch_attention(
            project,
            work_index,
            "The assigned repair context failed application validation.",
        );
    }
    let ticket = state.ticket.clone();
    let mut state = state.clone();
    let original = state.clone();
    state.status = ImplementationStatus::ChangesRequested;
    state.detail = format!(
        "Manager assigned scoped repair {} for blocking code-review finding {}. The repair worker must preserve the approved specification and produce fresh verification and independent review.",
        context.work_id, context.finding_id
    );
    state.manager_repair_context = Some(context);
    let state_dir =
        match implementation::state_dir_for_task(&project.state.repo_root, &ticket, Some(task_uid))
        {
            Ok(path) => path,
            Err(error) => {
                return mark_dispatch_attention(
                    project,
                    work_index,
                    &format!("Cannot locate implementation state for the repair: {error}"),
                );
            }
        };
    if let Err(error) = implementation::save(&state_dir, &state) {
        return mark_dispatch_attention(
            project,
            work_index,
            &format!("Cannot persist scoped repair assignment: {error:#}"),
        );
    }
    project.implementation_states.insert(ticket.clone(), state);
    project.planning_work[work_index].status = WorkStatus::InProgress;
    project.planning_work[work_index].detail.push_str(
        "\n\nThe application dispatched this blocking finding to the existing task clone. Completion requires fresh verification and independent review of the changed head.",
    );
    if let Err(error) = project.save_planning_work() {
        let rollback = implementation::save(&state_dir, &original);
        project
            .implementation_states
            .insert(ticket.clone(), original);
        return mark_dispatch_attention(
            project,
            work_index,
            &format!(
                "Could not persist the repair work status: {error}. Implementation assignment rollback: {}.",
                rollback
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "saved".into())
            ),
        );
    }
    project.activity.pending.push(format!(
        "Manager dispatched a scoped repair for blocking code-review finding {} on task {}.",
        project.planning_work[work_index]
            .code_review_disposition
            .as_ref()
            .map(|disposition| disposition.finding_id.as_str())
            .unwrap_or("unknown"),
        task_uid
    ));
    Some((ticket, work_id))
}

fn mark_dispatch_attention(
    project: &mut Project,
    index: usize,
    detail: &str,
) -> Option<(String, String)> {
    project.planning_work[index].status = WorkStatus::NeedsAttention;
    project.planning_work[index]
        .detail
        .push_str(&format!("\n\nBlocked: {detail}"));
    if let Err(error) = project.save_planning_work() {
        project.activity.pending.push(format!(
            "Code-review repair requires attention, but its disposition status could not be saved: {error}"
        ));
    }
    None
}
