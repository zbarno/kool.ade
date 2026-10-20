use super::*;

#[derive(Clone)]
pub(crate) struct CodeReviewArbitrationRequest {
    pub(crate) work_uid: String,
    pub(crate) ticket: String,
    pub(crate) context: implementation::CodeReviewArbitrationContext,
}

pub(crate) fn next_arbitration(project: &mut Project) -> Option<CodeReviewArbitrationRequest> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || !project.active_implementations.is_empty()
        || project.activity.code_review_arbitration.is_some()
    {
        return None;
    }
    let index = project.planning_work.iter().position(|work| {
        work.kind == WorkKind::CodeReviewDisposition
            && work.status == WorkStatus::NeedsAttention
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| {
                    disposition.status == CodeReviewDispositionStatusV1::Contested
                        && disposition.arbitration_attempts
                            < crate::core::planning_work::MAX_CODE_REVIEW_ARBITRATION_ATTEMPTS
                })
    })?;
    let work = project.planning_work[index].clone();
    let disposition = work.code_review_disposition.clone()?;
    if disposition.scope != Some(FindingScopeV1::Blocking) {
        return mark_attention(
            project,
            index,
            "Only a current blocking finding can enter arbitration.",
        );
    }
    let task_uid = work.parent_uid.as_deref()?;
    let state = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))?
        .clone();
    if state.pr_url.is_some() || state.merged_commit.is_some() {
        return mark_attention(
            project,
            index,
            "The task is already published; its frozen diff cannot be arbitrated.",
        );
    }
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle.task_for_planning_item(task_uid).ok().flatten()?;
    if task.phase != TaskPhase::Repairing
        || task.reviewed_code_subject_sha256.as_deref() != Some(disposition.subject_hash.as_str())
        || task.reviewed_code_head_sha.as_deref() != state.verified_head.as_deref()
    {
        return mark_attention(
            project,
            index,
            "The contested code-review subject is stale; refresh Manager evidence before arbitration.",
        );
    }
    let summary = match implementation::manager_findings_for_task(
        &project.state.repo_root,
        &state.ticket,
        task_uid,
    ) {
        Ok(Some(summary)) => summary,
        _ => {
            return mark_attention(
                project,
                index,
                "The validated current code-review receipt is unavailable.",
            );
        }
    };
    if summary.pass
        || summary.subject_hash != disposition.subject_hash
        || summary.head_sha != state.verified_head.as_deref().unwrap_or_default()
    {
        return mark_attention(
            project,
            index,
            "The contested finding no longer matches the current failing review.",
        );
    }
    let Some((finding, target)) =
        current_findings(project)
            .ok()?
            .into_iter()
            .find(|(item, target)| {
                item.id == disposition.evidence_ref
                    && target.task_uid == task_uid
                    && target.subject_hash == disposition.subject_hash
                    && target.finding_id == disposition.finding_id
                    && target.finding_fingerprint
                        == disposition
                            .finding_fingerprint
                            .as_deref()
                            .unwrap_or_default()
                    && target.scope == FindingScopeV1::Blocking
            })
    else {
        return mark_attention(
            project,
            index,
            "The cited finding is no longer present in the current review receipt.",
        );
    };
    let context = implementation::CodeReviewArbitrationContext {
        task_uid: task_uid.to_owned(),
        finding_id: target.finding_id,
        subject_hash: target.subject_hash,
        head_sha: summary.head_sha,
        diff_hash: summary.diff_hash,
        prior_reviewer_run_id: summary.reviewer_run_id,
        finding_fingerprint: target.finding_fingerprint,
        finding_record: finding.content,
        rationale: disposition.decision_reason,
    };
    let original = project.planning_work[index].clone();
    let disposition = project.planning_work[index]
        .code_review_disposition
        .as_mut()?;
    disposition.arbitration_attempts = disposition.arbitration_attempts.saturating_add(1);
    let attempt = disposition.arbitration_attempts;
    project.planning_work[index].status = WorkStatus::InProgress;
    project.planning_work[index].detail.push_str(&format!(
        "\n\nIndependent arbitration attempt {attempt} dispatched for reviewer {} on unchanged head {} and diff {}.",
        context.prior_reviewer_run_id, context.head_sha, context.diff_hash
    ));
    if project.save_planning_work().is_err() {
        project.planning_work[index] = original;
        return None;
    }
    Some(CodeReviewArbitrationRequest {
        work_uid: work.uid,
        ticket: state.ticket,
        context,
    })
}

pub(crate) fn finish_arbitration(
    project: &mut Project,
    request: &CodeReviewArbitrationRequest,
    result: Result<implementation::ManagerCodeReviewFindings, String>,
) {
    let Some(index) = project.planning_work.iter().position(|work| {
        work.uid == request.work_uid
            && work.kind == WorkKind::CodeReviewDisposition
            && work.status == WorkStatus::InProgress
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| {
                    disposition.status == CodeReviewDispositionStatusV1::Contested
                        && disposition.subject_hash == request.context.subject_hash
                        && disposition.finding_id == request.context.finding_id
                })
    }) else {
        project.activity.pending.push(format!(
            "Independent arbitration for code-review work {} completed, but its current contest record is missing or stale.",
            request.work_uid
        ));
        return;
    };
    let previous = project.planning_work[index].clone();
    let summary = match result {
        Ok(summary) => summary,
        Err(error) => {
            project.planning_work[index].status = WorkStatus::NeedsAttention;
            project.planning_work[index].detail.push_str(&format!(
                "\n\nIndependent arbitration attempt did not complete: {error}. The finding remains contested and unresolved."
            ));
            save_or_restore(project, index, previous);
            return;
        }
    };
    let current_task = LifecycleStore::new(&project.state.planning_store)
        .task_for_planning_item(&request.context.task_uid)
        .ok()
        .flatten();
    let current_receipt = current_task.as_ref().is_some_and(|task| {
        task.reviewed_code_head_sha.as_deref() == Some(request.context.head_sha.as_str())
            && task.reviewed_code_subject_sha256.as_deref() == Some(summary.subject_hash.as_str())
    });
    if !current_receipt
        || summary.head_sha != request.context.head_sha
        || summary.diff_hash != request.context.diff_hash
        || summary.reviewer_run_id == request.context.prior_reviewer_run_id
        || summary.subject_hash == request.context.subject_hash
    {
        project.planning_work[index].status = WorkStatus::NeedsAttention;
        project.planning_work[index]
            .detail
            .push_str("\n\nArbitration did not prove a new independent verdict on the exact frozen diff. The finding remains contested and unresolved.");
        save_or_restore(project, index, previous);
        return;
    }
    let finding_remains = summary.findings.iter().any(|finding| {
        finding.id == request.context.finding_id
            || implementation::finding_fingerprint(finding) == request.context.finding_fingerprint
    });
    let unable =
        summary.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable);
    let disposition = project.planning_work[index]
        .code_review_disposition
        .as_mut()
        .expect("validated contested work has a structured disposition");
    if !finding_remains && !unable {
        disposition.status = CodeReviewDispositionStatusV1::Resolved;
        project.planning_work[index].status = WorkStatus::Done;
        project.planning_work[index].detail.push_str(&format!(
            "\n\nIndependent arbitrator {} did not confirm finding {} on the exact frozen diff. The finding is resolved by fresh independent evidence, not by the Manager contest.",
            summary.reviewer_run_id, request.context.finding_id
        ));
    } else if unable {
        project.planning_work[index].status = WorkStatus::NeedsAttention;
        project.planning_work[index].detail.push_str(&format!(
            "\n\nIndependent arbitrator {} could not assess finding {}. The contest remains unresolved and bounded arbitration attempts are retained.",
            summary.reviewer_run_id, request.context.finding_id
        ));
    } else {
        disposition.status = CodeReviewDispositionStatusV1::Open;
        project.planning_work[index].status = WorkStatus::NeedsAttention;
        project.planning_work[index].detail.push_str(&format!(
            "\n\nIndependent arbitrator {} confirmed finding {}. Manager must re-triage the fresh receipt; no risk was waived.",
            summary.reviewer_run_id, request.context.finding_id
        ));
    }
    save_or_restore(project, index, previous);
}

fn mark_attention(
    project: &mut Project,
    index: usize,
    reason: &str,
) -> Option<CodeReviewArbitrationRequest> {
    project.planning_work[index].status = WorkStatus::NeedsAttention;
    project.planning_work[index]
        .detail
        .push_str(&format!("\n\nArbitration was not dispatched: {reason}"));
    let _ = project.save_planning_work();
    None
}

fn save_or_restore(project: &mut Project, index: usize, previous: Work) {
    if let Err(error) = project.save_planning_work() {
        project.planning_work[index] = previous;
        project.activity.pending.push(format!(
            "Code-review arbitration result could not be saved to its finding disposition: {error}"
        ));
    }
}
