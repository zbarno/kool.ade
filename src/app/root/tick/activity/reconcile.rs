pub(crate) fn reconcile_inactive_planning_work(project: &mut crate::app::session::Project) {
    let active_key = project
        .active_turn
        .as_ref()
        .and(project.active_planning_work.as_deref());
    let retry = std::mem::take(&mut project.activity.pending_planning_work);
    let mut updated = project.planning_work.clone();
    let active_arbitration_feature = project.activity.plan_review_feature.as_deref();
    let mut recovered = Vec::new();
    for work in &mut updated {
        if work.kind == crate::core::planning_work::WorkKind::CodeReviewDisposition
            && work.status == crate::core::planning_work::WorkStatus::InProgress
            && work
                .code_review_disposition
                .as_ref()
                .is_some_and(|disposition| {
                    disposition.status
                        == crate::core::planning_work::CodeReviewDispositionStatusV1::Contested
                })
            && project
                .activity
                .code_review_arbitration
                .as_ref()
                .is_none_or(|worker| worker.request.work_uid != work.uid)
        {
            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
            work.detail.push_str(
                "\n\nRecovered interrupted independent code-review arbitration. The contest remains unresolved and can be retried within its existing attempt limit.",
            );
            recovered.push(format!(
                "Recovered interrupted code-review arbitration: {}",
                work.title
            ));
        }
        if work.key.starts_with("task:manager-plan-contest:")
            && work.status == crate::core::planning_work::WorkStatus::InProgress
            && active_arbitration_feature != work.feature_id.as_deref()
        {
            if super::plan_review::lifecycle::recover_persisted_arbitration(project, work) {
                recovered.push(format!("Recovered persisted arbitration: {}", work.title));
            } else {
                super::plan_review::lifecycle::recover_interrupted_arbitration(work);
                recovered.push(format!("Recovered interrupted arbitration: {}", work.title));
            }
        }
    }
    if retry
        && let Some(active_key) = active_key
        && let Some(active) = updated.iter_mut().find(|work| work.key == active_key)
    {
        active.status = crate::core::planning_work::WorkStatus::InProgress;
        active.detail = "Planning in progress".into();
    }
    let mut excluded: std::collections::BTreeSet<_> = updated
        .iter()
        .filter(|work| {
            work.key.starts_with("task:manager-plan-contest:")
                && work.status == crate::core::planning_work::WorkStatus::InProgress
                && active_arbitration_feature == work.feature_id.as_deref()
        })
        .map(|work| work.uid.clone())
        .collect();
    excluded.extend(
        updated
            .iter()
            .filter(|work| {
                project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
            })
            .map(|work| work.uid.clone()),
    );
    let mut changes = crate::core::planning_work::reconcile_inactive_excluding(
        &project.state,
        &mut updated,
        active_key,
        &excluded,
    );
    changes.extend(recovered);
    if retry {
        changes.push("previously reconciled planning work".to_owned());
    }
    if changes.is_empty() {
        return;
    }

    project.planning_work = updated;
    match project.save_planning_work() {
        Ok(()) => {
            if retry {
                project
                    .activity
                    .pending
                    .retain(|event| !event.starts_with("Planning work could not be saved:"));
                project
                    .activity
                    .pending
                    .push("Previously reconciled planning status was saved.".into());
            } else {
                project.activity.pending.push(format!(
                    "Kool.ad/e reconciled planning work with no active turn: {}.",
                    changes.join("; ")
                ));
            }
        }
        Err(error) => {
            project.activity.pending_planning_work = true;
            if !project
                .activity
                .pending
                .iter()
                .any(|event| event.starts_with("Planning work could not be saved:"))
            {
                project.activity.pending.push(format!(
                    "Planning work could not be saved: {error}. Kool.ad/e will retry automatically."
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "reconcile/tests.rs"]
mod tests;
