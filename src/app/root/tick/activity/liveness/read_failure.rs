use crate::{app::session::Project, core::planning_work::WorkStatus};
use std::collections::BTreeSet;

/// Keep an unreadable lifecycle store from leaving an otherwise stranded
/// TaskIntent with only a transient activity message.
pub(super) fn reconcile(project: &mut Project, diagnostic: &str) {
    if lifecycle_controller_active(project) {
        notify(
            project,
            "TaskIntent liveness could not inspect lifecycle state",
            format!(
                "TaskIntent liveness is deferring recovery because a lifecycle controller is already active and lifecycle records could not be inspected: {}",
                crate::core::context_build::clip_bytes(diagnostic, 1_500)
            ),
        );
        return;
    }
    let roots = project
        .planning_work
        .iter()
        .filter(|work| {
            work.parent_uid.is_none()
                && work.task_intent_root_uid() == Some(work.uid.as_str())
                && work.task_intent_archived_at_ms.is_none()
                && !project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut updated = project.planning_work.clone();
    let mut changed = false;
    let mut created = false;

    for root in roots {
        let cycle_uid = root
            .task_intent_cycle_uid
            .clone()
            .or_else(|| {
                root.task_intent_cycles
                    .last()
                    .map(|cycle| cycle.uid.clone())
            })
            .unwrap_or_else(|| root.uid.clone());
        let children = updated
            .iter()
            .filter(|work| {
                work.uid != root.uid
                    && work.task_intent_uid.as_deref() == Some(root.uid.as_str())
                    && work.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
            })
            .collect::<Vec<_>>();
        if children.iter().any(|work| {
            matches!(
                work.status,
                WorkStatus::InReview | WorkStatus::NeedsAttention
            ) || (work.status == WorkStatus::Todo
                && matches!(
                    work.kind,
                    crate::core::planning_work::WorkKind::PlanConfirmation
                        | crate::core::planning_work::WorkKind::PlanReviewDisposition
                        | crate::core::planning_work::WorkKind::CodeReviewDisposition
                        | crate::core::planning_work::WorkKind::QaFindingDisposition
                ))
        }) {
            continue;
        }
        let owned_keys = children
            .iter()
            .map(|work| work.key.as_str())
            .chain(std::iter::once(root.key.as_str()))
            .collect::<BTreeSet<_>>();
        let task_paths = project
            .task_documents
            .iter()
            .filter(|document| {
                document.metadata.as_ref().is_some_and(|metadata| {
                    metadata.task_intent_uid.as_deref() == Some(root.uid.as_str())
                        && metadata
                            .task_intent_cycle_uid
                            .as_deref()
                            .is_none_or(|uid| uid == cycle_uid)
                })
            })
            .collect::<Vec<_>>();
        let active_worker = task_paths.iter().any(|document| {
            project.active_implementations.contains_key(&document.path)
                || project.queue.in_flight.contains(&document.path)
                || project.queue.waiting_for_capacity.contains(&document.path)
                || project.queue.current_ticket.as_ref() == Some(&document.path)
        });
        let mut held_task_lease = false;
        let mut lease_error = None;
        for document in &task_paths {
            let Some(metadata) = document.metadata.as_ref() else {
                continue;
            };
            match crate::core::task_claim::is_claimed_by_another_process(
                &project.state.planning_store.root,
                project.state.planning_store.project_id,
                &metadata.uid,
            ) {
                Ok(true) => held_task_lease = true,
                Ok(false) => {}
                Err(error) => lease_error = Some(error.to_string()),
            }
        }
        if project
            .active_planning_work
            .as_deref()
            .is_some_and(|key| owned_keys.contains(key))
            || active_worker
            || held_task_lease
            || project
                .task_turns
                .keys()
                .any(|key| owned_keys.contains(key.as_str()))
            || project.activity.manager.as_ref().is_some_and(|manager| {
                manager
                    .source_events()
                    .iter()
                    .any(|event| event == &root.uid || event == &root.key)
            })
        {
            continue;
        }

        let lease_unverified = lease_error.is_some();
        let lease_diagnostic = lease_error.map_or_else(String::new, |error| {
            format!(
                " The executable task lease could not be verified ({error}); do not resume that task until its OS lock can be checked."
            )
        });
        let message = format!(
            "Lifecycle feature/task records could not be inspected: {}. Refresh the workspace and retry liveness reconciliation; do not infer completion or resume an operation from unreadable state.",
            crate::core::context_build::clip_bytes(diagnostic, 1_500)
        ) + &lease_diagnostic;
        let mut recovery =
            crate::core::planning_work::liveness_recovery_task(&root, &cycle_uid, &message);
        if let Some(existing) = updated.iter_mut().find(|work| work.key == recovery.key) {
            if existing.status == WorkStatus::InProgress {
                existing.status = if existing.liveness_recovery_attempts
                    >= super::dispatch::MAX_AUTOMATIC_ATTEMPTS
                {
                    WorkStatus::NeedsAttention
                } else {
                    WorkStatus::Todo
                };
                existing.detail.push_str(
                    "\n\nThe previous liveness recovery was interrupted while lifecycle state remained unreadable; the same scoped action is retried within its attempt limit.",
                );
                changed = true;
            }
            if lease_unverified && existing.status != WorkStatus::NeedsAttention {
                existing.status = WorkStatus::NeedsAttention;
                changed = true;
            }
            if existing.detail != recovery.detail {
                existing.detail = recovery.detail;
                changed = true;
            }
        } else {
            recovery.status = if lease_unverified {
                WorkStatus::NeedsAttention
            } else {
                WorkStatus::Todo
            };
            updated.push(recovery);
            created = true;
            changed = true;
        }
    }

    if changed {
        let previous = std::mem::replace(&mut project.planning_work, updated);
        if let Err(error) = project.save_planning_work() {
            project.planning_work = previous;
            notify(
                project,
                "TaskIntent liveness findings could not be saved",
                format!("TaskIntent liveness findings could not be saved and will retry: {error}"),
            );
            return;
        }
        if created {
            notify(
                project,
                "A durable TaskIntent recovery task was created",
                "A durable TaskIntent recovery task was created because lifecycle records could not be inspected.".into(),
            );
        }
    }
    notify(
        project,
        "TaskIntent liveness could not inspect lifecycle state",
        format!(
            "TaskIntent liveness could not inspect lifecycle state and will retry: {}",
            crate::core::context_build::clip_bytes(diagnostic, 1_500)
        ),
    );
}

fn lifecycle_controller_active(project: &Project) -> bool {
    let activity = &project.activity;
    activity.plan_review.is_some()
        || activity.code_review_arbitration.is_some()
        || activity.clarification_planner.is_some()
        || activity.pending_clarification_planner.is_some()
        || activity.spec_reconciliation.is_some()
        || activity.feature_spec_reconciliation.is_some()
        || activity.runtime_qa_worker.is_some()
        || activity.runtime_qa_scenario_synthesis.is_some()
        || activity.runtime_qa_applicability_review.is_some()
        || activity.runtime_recipe_worker.is_some()
        || activity.dependency_manager.is_some()
        || !activity.pending_dependency_reviews.is_empty()
}

fn notify(project: &mut Project, prefix: &str, message: String) {
    if !project
        .activity
        .pending
        .iter()
        .any(|pending| pending.starts_with(prefix))
    {
        project.activity.pending.push(message);
    }
}
