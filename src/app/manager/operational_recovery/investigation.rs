//! Own failed Agent investigations even when no submitted user reply exists.
use super::*;
mod context;
const ACTIVE_DETAIL: &str = "Manager is monitoring the retrying investigation; a durable result is required before completion.";

pub(crate) fn enqueue(
    project: &mut Project,
    key: &str,
    source_uid: &str,
    error: &str,
    generation: Option<u8>,
) {
    let alias = project
        .state
        .items
        .iter()
        .find(|item| item.id == key)
        .map(|item| item.conversation_key().to_owned());
    let Some(source) = alias.as_deref().and_then(|alias| source(project, alias)) else {
        return;
    };
    if source.uid != source_uid {
        return;
    }
    let mut task = operational_recovery::task(
        &source,
        &format!("investigation:{}", source.uid),
        FailureKind::Execution,
        crate::core::context_build::clip_bytes(error, 16 * 1024),
    );
    let recovery = task.operational_recovery.as_mut().unwrap();
    recovery.autonomous_investigation = true;
    recovery.next_attempt_ms = chrono::Utc::now().timestamp_millis() + 30_000;
    task.detail = "Manager owns triage of the failed Agent investigation. A fresh investigation will retry after the cooldown using current evidence; no user answer is required.".into();
    let original = project.planning_work.clone();
    if let Some(existing) = project.planning_work.iter_mut().find(|w| w.uid == task.uid) {
        let recovery = existing.operational_recovery.as_mut().unwrap();
        if recovery.attempts > 0 && generation != Some(recovery.investigation_claims) {
            return;
        }
        recovery.evidence = crate::core::context_build::clip_bytes(error, 16 * 1024);
        if recovery.attempts > 0 {
            recovery.repair_error = Some(error.to_owned());
            existing.status = WorkStatus::NeedsAttention;
            existing.detail = format!(
                "The bounded investigation retry failed. Manager retains the evidence and ownership; repair the reported execution or response failure before another attempt. {error}"
            );
        }
    } else {
        project.planning_work.push(task);
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.queue.last_error =
            format!("Manager could not record investigation triage: {error}");
    } else {
        project.activity.pending.push("Manager created or updated an investigation triage task; the worker failure is internally owned.".into());
    }
}

pub(crate) fn blocked(project: &Project, source_uid: Option<&str>) -> bool {
    project.planning_work.iter().any(|work| {
        work.operational_recovery.as_ref().is_some_and(|r| {
            r.autonomous_investigation
                && Some(r.source_uid.as_str()) == source_uid
                && (r.attempts == 0
                    || r.investigation_claims >= 2
                    || r.repair_error.is_some()
                    || chrono::Utc::now().timestamp_millis() < r.next_attempt_ms
                    || [&work.uid, &r.source_uid].iter().any(|uid| {
                        project
                            .cancelled_work
                            .contains(&crate::persistence::cancelled_work::planning_id(uid))
                    })
                    || r.expected_feature_uid.as_ref().is_some_and(|uid| {
                        project
                            .cancelled_work
                            .contains(&crate::persistence::cancelled_work::planning_id(uid))
                    }))
        })
    })
}

pub(crate) fn claim(project: &mut Project, item_id: &str) -> bool {
    let Some(item) = project.state.items.iter().find(|item| item.id == item_id) else {
        return false;
    };
    let uid = item.uid.clone();
    let Some(index) = project.planning_work.iter().position(|work| {
        work.operational_recovery
            .as_ref()
            .is_some_and(|r| r.autonomous_investigation && Some(&r.source_uid) == uid.as_ref())
    }) else {
        return true;
    };
    if blocked(project, uid.as_deref()) {
        return false;
    }
    let original = project.planning_work.clone();
    let task = &mut project.planning_work[index];
    if task
        .operational_recovery
        .as_ref()
        .unwrap()
        .expected_feature_uid
        != item.feature_uid
    {
        return false;
    }
    task.operational_recovery
        .as_mut()
        .unwrap()
        .investigation_claims += 1;
    task.status = WorkStatus::InProgress;
    task.detail = ACTIVE_DETAIL.into();
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.queue.last_error =
            format!("Investigation retry ownership could not be checkpointed: {error}");
        false
    } else {
        true
    }
}

pub(super) fn advance(project: &mut Project, index: usize) -> bool {
    if context::rearm(project, index) {
        return true;
    }
    let recovery = project.planning_work[index]
        .operational_recovery
        .clone()
        .unwrap();
    let original = project.planning_work.clone();
    let source = project
        .state
        .items
        .iter()
        .chain(&project.state.resolved_items)
        .find(|item| item.uid.as_deref() == Some(recovery.source_uid.as_str()));
    let cancelled = [&project.planning_work[index].uid, &recovery.source_uid]
        .iter()
        .any(|uid| {
            project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(uid))
        })
        || recovery.expected_feature_uid.as_ref().is_some_and(|uid| {
            project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(uid))
        });
    let terminal = source.is_some_and(|item| {
        item.status != crate::domain::ItemStatus::Open
            || item.authority != crate::domain::Authority::Agent
    });
    let ownership =
        source.map(|item| crate::core::investigation::try_claim(&project.state, &item.id));
    let globally_owned = ownership
        .as_ref()
        .is_some_and(|result| matches!(result, Ok(None)));
    let mut dispatch = None;
    let (status, detail) = if cancelled {
        (
            WorkStatus::Done,
            "Investigation recovery stopped because its source was cancelled.".into(),
        )
    } else if terminal {
        (WorkStatus::Done, "Verified: the investigation has a durable resolution or an explicit decision escalation. Manager triage is complete.".into())
    } else if source.is_none()
        || source.is_some_and(|item| item.feature_uid != recovery.expected_feature_uid)
    {
        (WorkStatus::NeedsAttention, "Investigation recovery blocked: the original item or immutable feature link changed. Reconcile that identity before retrying.".into())
    } else if let Some(Err(error)) = &ownership {
        (
            WorkStatus::NeedsAttention,
            format!(
                "Manager cannot verify investigation ownership: {error}. Restore access to the private execution lock before retrying."
            ),
        )
    } else if recovery.repair_error.is_some() {
        (
            WorkStatus::NeedsAttention,
            format!(
                "Investigation retry budget exhausted. Repair the recorded failure before another attempt: {}",
                recovery.evidence
            ),
        )
    } else if globally_owned
        || project
            .investigation
            .as_ref()
            .is_some_and(|worker| source.is_some_and(|item| worker.item_id == item.id))
    {
        (WorkStatus::InProgress, ACTIVE_DETAIL.into())
    } else if recovery.investigation_claims >= 2 {
        (WorkStatus::NeedsAttention, "Investigation recovery was interrupted twice. Manager retained the source and evidence; inspect the execution failure before authorizing further recovery.".into())
    } else if !project.queue.auto_plan {
        (WorkStatus::NeedsAttention, "Investigation recovery is paused because Auto Plan is off. Enable Auto Plan to resume this application-owned investigation.".into())
    } else if recovery.attempts == 0
        && chrono::Utc::now().timestamp_millis() >= recovery.next_attempt_ms
    {
        dispatch = source.map(|item| item.id.clone());
        (WorkStatus::Todo, "Manager scheduled one fresh investigation against current planning state and repository evidence.".into())
    } else {
        (WorkStatus::Todo, "Investigation recovery is queued behind its cooldown or the current Agent investigation.".into())
    };
    project.planning_work[index].status = status;
    project.planning_work[index].detail = detail;
    if dispatch.is_some() {
        project.planning_work[index]
            .operational_recovery
            .as_mut()
            .unwrap()
            .attempts = 1;
    }
    if project.planning_work == original {
        return false;
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.queue.last_error =
            format!("Investigation recovery could not checkpoint its next action: {error}");
    } else if let Some(key) = dispatch {
        project.investigation_attempted.remove(&key);
        project.activity.pending.push(format!(
            "Manager triage scheduled the bounded investigation retry for {key}."
        ));
    }
    true
}

/// Fence terminal callbacks against the durable source identity and attempt,
/// before adopting activity, results, attempted flags, or Manager events.
pub(crate) fn callback_state(
    project: &Project,
    item_id: &str,
    source_uid: Option<&str>,
    generation: Option<u8>,
) -> Result<Option<(crate::core::state::PlannerState, Vec<Work>)>, String> {
    let Some(uid) = source_uid else {
        return Ok(None);
    };
    let state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .map_err(|error| error.to_string())?;
    if !state
        .items
        .iter()
        .chain(&state.resolved_items)
        .any(|item| item.id == item_id && item.uid.as_deref() == Some(uid))
    {
        return Ok(None);
    }
    let (work, _) = crate::core::planning_work::load_expected(
        &state.planning_store,
        &state.baseline_planning_revision,
    )
    .map_err(|error| error.to_string())?;
    if let Some(recovery) = work
        .iter()
        .filter_map(|work| work.operational_recovery.as_ref())
        .find(|r| r.autonomous_investigation && r.source_uid == uid)
        && recovery.attempts > 0
        && generation != Some(recovery.investigation_claims)
    {
        return Ok(None);
    }
    Ok(Some((state, work)))
}
