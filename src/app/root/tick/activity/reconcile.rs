pub(super) fn reconcile_inactive_planning_work(project: &mut crate::app::session::Project) {
    let active_key = project
        .active_turn
        .as_ref()
        .and(project.active_planning_work.as_deref());
    let retry = std::mem::take(&mut project.activity.pending_planning_work);
    let mut updated = project.planning_work.clone();
    if retry
        && let Some(active_key) = active_key
        && let Some(active) = updated.iter_mut().find(|work| work.key == active_key)
    {
        active.status = crate::core::planning_work::WorkStatus::InProgress;
        active.detail = "Planning in progress".into();
    }
    let cancelled = updated
        .iter()
        .filter(|work| {
            project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        })
        .map(|work| work.uid.clone())
        .collect();
    let mut changes = crate::core::planning_work::reconcile_inactive_excluding(
        &project.state,
        &mut updated,
        active_key,
        &cancelled,
    );
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
