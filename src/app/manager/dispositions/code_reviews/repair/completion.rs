use super::*;
#[cfg(test)]
#[path = "completion/tests.rs"]
mod tests;

pub(crate) fn finish_repair(project: &mut Project, ticket: &str) {
    let Some(mut state) = project.implementation_states.get(ticket).cloned() else {
        return;
    };
    let Some(context) = state.manager_repair_context.clone() else {
        return;
    };
    if context.is_runtime_qa() {
        finish_runtime_qa_repair_review(project, ticket, &context);
        return;
    }
    let review = implementation::manager_findings_for_task(
        &project.state.repo_root,
        ticket,
        &context.task_uid,
    );
    let review = match review {
        Ok(Some(review)) => review,
        Ok(None) => {
            mark_attention(
                project,
                &context,
                "The worker completed without a validated independent review receipt for its current head.",
            );
            return;
        }
        Err(error) => {
            mark_attention(
                project,
                &context,
                &format!("The current independent review receipt is invalid: {error:#}"),
            );
            return;
        }
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let current_task = lifecycle
        .task_for_planning_item(&context.task_uid)
        .ok()
        .flatten();
    let review_is_current = current_task.as_ref().is_some_and(|task| {
        review_matches_new_head(
            &context,
            &review.head_sha,
            &review.subject_hash,
            task.reviewed_code_head_sha.as_deref(),
            task.reviewed_code_subject_sha256.as_deref(),
        )
    });
    if !review_is_current {
        mark_attention(
            project,
            &context,
            "The review receipt does not prove independent review of a new current head and subject; the repair remains unresolved.",
        );
        return;
    };
    let resolved = repair_finding_resolved(&context, &review);
    let Some(index) = project.planning_work.iter().position(|work| {
        work.uid == context.work_id
            && work.kind == WorkKind::CodeReviewDisposition
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| {
                    disposition.status == CodeReviewDispositionStatusV1::AssignedRepair
                        && disposition.finding_id == context.finding_id
                        && disposition.subject_hash == context.reviewed_subject_hash
                        && disposition.owner_work_id == context.work_id
                })
    }) else {
        project.activity.pending.push(format!(
            "Fresh code review completed for task {ticket}, but its assigned repair record {} is missing.",
            context.work_id
        ));
        return;
    };
    let old_work = project.planning_work[index].clone();
    let Some(disposition) = project.planning_work[index]
        .code_review_disposition
        .as_mut()
    else {
        return;
    };
    if resolved {
        disposition.status = CodeReviewDispositionStatusV1::Resolved;
        project.planning_work[index].status = WorkStatus::Done;
        project.planning_work[index].detail.push_str(&format!(
            "\n\nResolved by passing independent code review on head {}. The reviewed finding is absent from the fresh receipt.",
            review.head_sha
        ));
    } else {
        disposition.status = CodeReviewDispositionStatusV1::Open;
        project.planning_work[index].status = WorkStatus::NeedsAttention;
        project.planning_work[index].detail.push_str(&format!(
            "\n\nThe fresh independent review on head {} did not resolve finding {}. Manager must re-evaluate the current receipt; another repair is not dispatched automatically.",
            review.head_sha, context.finding_id
        ));
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work[index] = old_work;
        project.activity.pending.push(format!(
            "Fresh code review completed, but the repair disposition could not be updated: {error}"
        ));
        return;
    }
    state.manager_repair_context = None;
    state.detail.push_str(if resolved {
        "\nThe Manager-assigned code-review repair was resolved by a fresh passing review."
    } else {
        "\nThe Manager-assigned code-review repair needs renewed Manager triage after fresh review."
    });
    let state_dir = match implementation::state_dir_for_task(
        &project.state.repo_root,
        ticket,
        Some(&context.task_uid),
    ) {
        Ok(path) => path,
        Err(error) => {
            project.planning_work[index] = old_work;
            let _ = project.save_planning_work();
            project.activity.pending.push(format!(
                "Cannot finalize code-review repair state because its implementation record cannot be located: {error}"
            ));
            return;
        }
    };
    if let Err(error) = implementation::save(&state_dir, &state) {
        project.planning_work[index] = old_work;
        let _ = project.save_planning_work();
        project.activity.pending.push(format!(
            "Cannot finalize code-review repair state; the assignment was retained for recovery: {error:#}"
        ));
        return;
    }
    project
        .implementation_states
        .insert(ticket.to_owned(), state);
    project.activity.pending.push(if resolved {
        format!("Independent review resolved the assigned code-review finding for task {ticket}.")
    } else {
        format!("The assigned code-review repair remains unresolved for task {ticket}; Manager triage is required.")
    });
}

fn finish_runtime_qa_repair_review(
    project: &mut Project,
    ticket: &str,
    context: &crate::core::implementation::ManagerRepairContextV1,
) {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let current = lifecycle
        .task_for_planning_item(&context.task_uid)
        .ok()
        .flatten();
    let review_is_current = current.as_ref().is_some_and(|task| {
        task.phase == TaskPhase::AwaitingRuntimeQa
            && task.reviewed_code_head_sha.as_deref()
                == project
                    .implementation_states
                    .get(ticket)
                    .and_then(|state| state.verified_head.as_deref())
            && task.reviewed_code_head_sha.as_deref() != Some(context.reviewed_head_sha.as_str())
    });
    let Some(index) = project.planning_work.iter().position(|work| {
        work.uid == context.work_id
            && work.kind == WorkKind::QaFindingDisposition
            && work.parent_uid.as_deref() == Some(context.task_uid.as_str())
    }) else {
        project.activity.pending.push(format!(
            "Fresh code review completed for task {ticket}, but its Runtime QA repair record {} is missing.",
            context.work_id
        ));
        return;
    };
    if !review_is_current {
        project.planning_work[index].status = WorkStatus::NeedsAttention;
        project.planning_work[index].detail.push_str(
            "\n\nThe changed head did not reach the fresh-review-passed Runtime QA phase; Manager must re-evaluate the current task state.",
        );
        let _ = project.save_planning_work();
        return;
    }
    project.planning_work[index].status = WorkStatus::InReview;
    project.planning_work[index].detail.push_str(
        "\n\nFresh independent code review passed on the changed head. The repair remains unresolved until a new Runtime QA report passes.",
    );
    if let Err(error) = project.save_planning_work() {
        project.activity.pending.push(format!(
            "Fresh code review passed for Runtime QA repair {}, but its board state could not be saved: {error}",
            context.work_id
        ));
    }
}

fn repair_finding_resolved(
    context: &ManagerRepairContextV1,
    review: &implementation::ManagerCodeReviewFindings,
) -> bool {
    review.pass
        && !review.findings.iter().any(|finding| {
            finding.id == context.finding_id
                || context.finding_fingerprint.as_deref()
                    == Some(implementation::finding_fingerprint(finding).as_str())
        })
}

fn review_matches_new_head(
    context: &ManagerRepairContextV1,
    review_head: &str,
    review_subject: &str,
    current_head: Option<&str>,
    current_subject: Option<&str>,
) -> bool {
    current_head == Some(review_head)
        && current_subject == Some(review_subject)
        && review_head != context.reviewed_head_sha
        && review_subject != context.reviewed_subject_hash
}

fn mark_attention(project: &mut Project, context: &ManagerRepairContextV1, reason: &str) {
    let Some(index) = project.planning_work.iter().position(|work| {
        work.uid == context.work_id
            && work.kind == WorkKind::CodeReviewDisposition
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| disposition.owner_work_id == context.work_id)
    }) else {
        project.activity.pending.push(format!(
            "Code-review repair {} requires attention, but its board record is missing: {reason}",
            context.work_id
        ));
        return;
    };
    let previous = project.planning_work[index].clone();
    project.planning_work[index].status = WorkStatus::NeedsAttention;
    project.planning_work[index]
        .detail
        .push_str(&format!("\n\nRepair stopped: {reason}"));
    if let Err(error) = project.save_planning_work() {
        project.planning_work[index] = previous;
        project.activity.pending.push(format!(
            "Code-review repair {} requires attention, but its board state could not be saved: {error}",
            context.work_id
        ));
    }
}
