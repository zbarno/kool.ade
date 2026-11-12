use crate::app::session::Project;
use crate::core::{
    lifecycle::LifecycleStore,
    planning_work::WorkStatus,
    task_intent::{TaskIntent, TaskIntentLifecycle},
};
use std::collections::BTreeSet;

mod dispatch;
pub(super) use dispatch::next_recovery;
mod projection;
mod read_failure;

#[cfg(test)]
mod tests;

pub(super) fn reconcile(project: &mut Project) {
    let planning_store = project.state.planning_store.clone();
    let lifecycle = LifecycleStore::new(&planning_store);
    let (features, tasks) = match (lifecycle.list_features(), lifecycle.list_tasks()) {
        (Ok(features), Ok(tasks)) => (features, tasks),
        (features, tasks) => {
            let errors = [features.err(), tasks.err()]
                .into_iter()
                .flatten()
                .map(|error| error.to_string())
                .collect::<Vec<_>>();
            read_failure::reconcile(project, &errors.join("; "));
            return;
        }
    };
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
    let mut created_recovery = false;

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
        let owned_keys = updated
            .iter()
            .filter(|work| {
                work.uid == root.uid
                    || (work.task_intent_uid.as_deref() == Some(root.uid.as_str())
                        && work.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str()))
            })
            .map(|work| work.key.clone())
            .collect::<BTreeSet<_>>();
        let children = updated
            .iter()
            .filter(|work| {
                work.uid != root.uid
                    && work.task_intent_uid.as_deref() == Some(root.uid.as_str())
                    && work.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
            })
            .collect::<Vec<_>>();
        let child_statuses = children.iter().map(|work| work.status).collect::<Vec<_>>();
        let feature_uid = root
            .feature_uid
            .as_deref()
            .or_else(|| children.iter().find_map(|work| work.feature_uid.as_deref()));
        let feature = feature_uid.and_then(|uid| features.iter().find(|f| f.feature_uid == uid));
        let task_records = feature_uid
            .map(|uid| {
                tasks
                    .iter()
                    .filter(|task| task.feature_uid == uid)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let task_phases = match projection::task_phases(&lifecycle, &task_records) {
            Ok(phases) => phases,
            Err(error) => {
                push_pending_once(
                    project,
                    "TaskIntent liveness could not inspect lifecycle publication state",
                    format!(
                        "TaskIntent liveness could not inspect lifecycle publication state for feature {}; it will retry: {error}",
                        feature_uid.unwrap_or("unknown")
                    ),
                );
                continue;
            }
        };
        let deployment = if let Some(uid) = feature_uid {
            match (
                lifecycle.load_deployment_evidence(uid),
                lifecycle.load_current_implementation(uid),
            ) {
                (Ok(Some(receipt)), Ok(Some(current))) => Some((receipt, current)),
                (Ok(_), Ok(_)) => None,
                (deployment, implementation) => {
                    let errors = [deployment.err(), implementation.err()]
                        .into_iter()
                        .flatten()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>();
                    push_pending_once(
                        project,
                        "TaskIntent liveness could not inspect delivery evidence",
                        format!(
                            "TaskIntent liveness could not inspect delivery evidence for feature {uid}; it will retry: {}",
                            errors.join("; ")
                        ),
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let decisions = project
            .state
            .items
            .iter()
            .chain(&project.state.resolved_items)
            .filter(|item| {
                item.task_intent_uid.as_deref() == Some(root.uid.as_str())
                    && item.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
            })
            .map(|item| item.status)
            .collect::<Vec<_>>();
        let has_actionable_decision = project.state.items.iter().any(|item| {
            item.task_intent_uid.as_deref() == Some(root.uid.as_str())
                && item.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
                && item.status == crate::domain::item::ItemStatus::Open
                && item.blocked_by.is_empty()
        });
        let has_actionable_child = children.iter().any(|work| {
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
        });
        let intent = TaskIntent::project_with_decisions(
            &root,
            &child_statuses,
            &decisions,
            feature.map(|feature| feature.phase),
            &task_phases,
            deployment
                .as_ref()
                .map(|(receipt, current)| (receipt, current)),
            false,
        );
        let task_dispatch = dispatch::has_dispatchable_task(project, &task_records);
        if intent.as_ref().is_some_and(|intent| {
            matches!(
                intent.lifecycle,
                TaskIntentLifecycle::Complete
                    | TaskIntentLifecycle::Verified
                    | TaskIntentLifecycle::Merged
                    | TaskIntentLifecycle::Deployed
                    | TaskIntentLifecycle::Cancelled
            )
        }) {
            continue;
        }
        if has_actionable_decision
            || has_actionable_child
            || project
                .active_planning_work
                .as_ref()
                .is_some_and(|key| owned_keys.contains(key))
            || project
                .task_turns
                .keys()
                .any(|key| owned_keys.contains(key))
            || project.activity.manager.as_ref().is_some_and(|manager| {
                manager
                    .source_events()
                    .iter()
                    .any(|event| event == &root.uid || event == &root.key)
            })
            || task_dispatch
                .as_ref()
                .is_ok_and(|dispatchable| *dispatchable)
        {
            continue;
        }

        let key = format!("task:manager-liveness:{}:{cycle_uid}", root.uid);
        if let Some(index) = updated.iter().position(|work| work.key == key) {
            if updated[index].status == WorkStatus::InProgress {
                let recovery = &mut updated[index];
                if recovery.liveness_recovery_attempts >= dispatch::MAX_AUTOMATIC_ATTEMPTS {
                    recovery.status = WorkStatus::NeedsAttention;
                    recovery.detail.push_str("\n\nAutomatic recovery reached its bounded attempt limit after an interrupted Planner operation. Review the blocker and choose the supported next step.");
                } else {
                    recovery.status = WorkStatus::Todo;
                    recovery.detail.push_str("\n\nThe previous Planner recovery was interrupted before a durable outcome; the same scoped recovery is queued for one bounded retry.");
                }
                changed = true;
            }
            if updated[index].status == WorkStatus::Done
                && updated
                    .iter()
                    .find(|work| work.uid == root.uid)
                    .is_some_and(|work| work.status != WorkStatus::NeedsAttention)
            {
                let root = updated
                    .iter_mut()
                    .find(|work| work.uid == root.uid)
                    .unwrap();
                root.status = WorkStatus::NeedsAttention;
                root.detail.push_str("\n\nThe liveness recovery task was completed, but this cycle remains stalled. Review its recorded blocker and choose the supported resolution.");
                changed = true;
            }
            continue;
        }
        let lease_diagnostic = task_dispatch
            .err()
            .map(|error| format!(" The running task's lease could not be verified: {error}."))
            .unwrap_or_default();
        let diagnosis = format!(
            "TaskIntent {} cycle {} is {}, but no active turn, held task lease, actionable human decision, open child operation, or eligible task dispatcher was found.",
            root.uid,
            cycle_uid,
            intent.map_or("unknown", |intent| intent.lifecycle.label())
        ) + &lease_diagnostic;
        updated.push(crate::core::planning_work::liveness_recovery_task(
            &root, &cycle_uid, &diagnosis,
        ));
        created_recovery = true;
        changed = true;
    }

    if changed {
        let previous = std::mem::replace(&mut project.planning_work, updated);
        if let Err(error) = project.save_planning_work() {
            project.planning_work = previous;
            push_pending_once(
                project,
                "TaskIntent liveness findings could not be saved",
                format!("TaskIntent liveness findings could not be saved and will retry: {error}"),
            );
        } else if created_recovery {
            push_pending_once(
                project,
                "A durable TaskIntent recovery task was created",
                "A durable TaskIntent recovery task was created for stalled work.".into(),
            );
        }
    }
}

fn push_pending_once(project: &mut Project, prefix: &str, message: String) {
    if !project
        .activity
        .pending
        .iter()
        .any(|pending| pending.starts_with(prefix))
    {
        project.activity.pending.push(message);
    }
}
