//! Task queue scheduling and implementation lifecycle orchestration.
use super::*;

impl KooladeApp {
    pub(super) fn advance_auto_queue(&mut self) {
        let next = if let Screen::Connected(project) = &mut self.screen {
            project.queue.waiting_for_capacity.clear();
            let cancelled = project.cancelled_task_paths();
            let recovery = project
                .queue
                .recoverable_tickets(&project.task_documents)
                .into_iter()
                .filter(|ticket| !cancelled.contains(ticket))
                .collect::<Vec<_>>();
            if !recovery.is_empty() {
                if project.queue_lock.is_none() {
                    match crate::core::implementation_queue::Queue::acquire(
                        &project.state.repo_root,
                    ) {
                        Ok(lock) => project.queue_lock = Some(lock),
                        Err(_) => return,
                    }
                }
                let previous = project.queue.clone();
                project.queue.schedule_recovery(&recovery);
                if let Err(error) = project.queue.save(&project.state.repo_root) {
                    project.queue = previous;
                    project.queue.last_error =
                        format!("Cannot persist automatic recovery: {error}");
                    return;
                }
                project.activity.pending.push(format!("Automatically resuming {} task(s) after recoverable orchestration failures; preserved work and verification will be reused.", recovery.len()));
            }
            if !project.queue.auto_build {
                return;
            }
            if !project.queue.running {
                let mut excluded = project
                    .active_implementations
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                excluded.extend(cancelled.iter().cloned());
                excluded.extend(project.queue.blocked.keys().cloned());
                for doc in &project.task_documents {
                    if let Some(id) = doc
                        .text
                        .lines()
                        .find_map(|line| line.strip_prefix("Feature ID: "))
                        && !crate::core::workflow::feature_approved(
                            &project.state.planning_store,
                            &project.state.workflow,
                            id,
                        )
                    {
                        excluded.insert(doc.path.clone());
                    }
                }
                let running = project
                    .active_implementations
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                if !matches!(
                    crate::core::implementation_queue::next_ready_ticket_with_running_scopes(
                        &project.task_documents,
                        &project.implementation_states,
                        &excluded,
                        &running,
                    ),
                    Ok(Some(_))
                ) {
                    return;
                }
                project.queue.running = true;
            }
            if project.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&project.state.repo_root) {
                    Ok(lock) => project.queue_lock = Some(lock),
                    Err(error) => {
                        project.queue.running = false;
                        project.queue.last_error = error.to_string();
                        return;
                    }
                }
            }
            // A process can stop after Pi saved a blocked report but before
            // Koolade records the terminal state. Recover that checkpoint
            // before the durable Auto queue selects the same ticket again.
            let interrupted = project
                .queue
                .in_flight
                .iter()
                .filter(|ticket| !project.active_implementations.contains_key(*ticket))
                .filter(|ticket| {
                    project
                        .implementation_states
                        .get(*ticket)
                        .is_some_and(|state| state.status != ImplementationStatus::Completed)
                })
                .cloned()
                .collect::<Vec<_>>();
            let mut recovered = false;
            for ticket in interrupted {
                if let Some(detail) = crate::core::implementation::latest_external_blocker(
                    &project.state.repo_root,
                    &ticket,
                ) {
                    project.queue.in_flight.remove(&ticket);
                    project.queue.blocked.insert(
                        ticket.clone(),
                        crate::core::implementation::Failure::new(
                            crate::core::implementation::FailureKind::ExternalPrerequisite,
                            crate::core::implementation::RecoveryDisposition::UserAction,
                            detail,
                        ),
                    );
                    recovered = true;
                    project.activity.pending.push(format!(
                        "{ticket}: restored the saved external blocker; user action is required before resuming."
                    ));
                }
            }
            if recovered && let Err(error) = project.queue.save(&project.state.repo_root) {
                project.queue.last_error =
                    format!("Cannot persist recovered task checkpoints: {error}");
                return;
            }
            let mut excluded = project
                .active_implementations
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            excluded.extend(project.cancelled_task_paths());
            excluded.extend(project.queue.blocked.keys().cloned());
            // Remember WHY a ticket is excluded so a parked queue can tell the
            // operator which approval is missing or lapsed instead of sitting
            // silent (the CHG-003 deadlock arrived invisibly this way).
            let mut approval_notes = std::collections::BTreeSet::new();
            for doc in &project.task_documents {
                if project
                    .implementation_states
                    .get(&doc.path)
                    .is_some_and(|state| state.status == ImplementationStatus::Completed)
                {
                    continue;
                }
                if let Some(id) = doc
                    .text
                    .lines()
                    .find_map(|line| line.strip_prefix("Feature ID: "))
                    && !crate::core::workflow::feature_approved(
                        &project.state.planning_store,
                        &project.state.workflow,
                        id,
                    )
                {
                    let reason = if project.state.workflow.approved_features.contains_key(id) {
                        "approval lapsed after the feature document changed — re-run Approve feature for implementation"
                    } else {
                        "no recorded approval — run Approve feature for implementation"
                    };
                    excluded.insert(doc.path.clone());
                    approval_notes.insert(format!("{0} (feature {id}): {reason}", doc.path));
                }
            }
            let running = project
                .active_implementations
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            let choice = crate::core::implementation_queue::next_ready_ticket_with_running_scopes(
                &project.task_documents,
                &project.implementation_states,
                &excluded,
                &running,
            );
            if project.active_implementations.len() >= project.queue.max_parallel.clamp(1, 8) {
                project.queue.waiting_for_capacity = choice
                    .as_ref()
                    .ok()
                    .and_then(|choice| choice.clone())
                    .into_iter()
                    .collect();
                return;
            }
            match choice {
                Ok(Some(ticket)) => Some(ticket),
                Ok(None) => {
                    if !project.active_implementations.is_empty() {
                        return;
                    }
                    project.queue.running = false;
                    project.queue.current_ticket = None;
                    project.queue.in_flight.clear();
                    let mut stall_notes = project
                        .queue
                        .blocked
                        .iter()
                        .map(|(ticket, failure)| format!("{ticket}: {failure}"))
                        .collect::<Vec<_>>();
                    stall_notes.extend(approval_notes);
                    project.queue.last_error = stall_notes.join("\n");
                    if let Err(error) = project.queue.save(&project.state.repo_root) {
                        project.queue.last_error = error.to_string();
                    }
                    project.queue_lock = None;
                    None
                }
                Err(error) => {
                    let mut reasons = vec![error];
                    reasons.extend(
                        project
                            .queue
                            .blocked
                            .iter()
                            .map(|(ticket, failure)| format!("{ticket}: {failure}")),
                    );
                    reasons.extend(approval_notes);
                    project.queue.last_error = reasons.join("\n");
                    if project.active_implementations.is_empty() {
                        project.queue.running = false;
                        let _ = project.queue.save(&project.state.repo_root);
                        project.queue_lock = None;
                    }
                    None
                }
            }
        } else {
            None
        };
        if let Some(ticket) = next {
            self.start_implementation(ticket.clone(), false);
            if let Screen::Connected(p) = &mut self.screen
                && !p.active_implementations.contains_key(&ticket)
            {
                p.queue.blocked.insert(
                    ticket,
                    crate::core::implementation::Failure::other(p.queue.last_error.clone()),
                );
                if p.queue_lock.is_some() {
                    let _ = p.queue.save(&p.state.repo_root);
                }
            }
        }
    }

    pub(super) fn advance_auto_publish(&mut self) {
        let ready = match &self.screen {
            Screen::Connected(project)
                if project.queue.auto_publish
                    && project.active_implementations.len()
                        < project.queue.max_parallel.clamp(1, 8) =>
            {
                project
                    .implementation_states
                    .iter()
                    .filter(|(ticket, state)| {
                        state.status == ImplementationStatus::ReadyToPublish
                            && !project.task_cancelled(ticket)
                            && !project.active_implementations.contains_key(*ticket)
                            && !project.queue.blocked.contains_key(*ticket)
                    })
                    .map(|(ticket, _)| ticket.clone())
                    .collect::<Vec<_>>()
            }
            _ => return,
        };
        for ticket in ready {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            if project.active_implementations.len() >= project.queue.max_parallel.clamp(1, 8) {
                break;
            }
            let failure = crate::core::implementation::Failure::new(
                crate::core::implementation::FailureKind::ExternalPrerequisite,
                crate::core::implementation::RecoveryDisposition::UserAction,
                "Verified work is saved locally. Review it, then choose Share verified work for review before Kool.ad/e pushes a branch or creates a pull request.",
            );
            project.queue.blocked.insert(ticket.clone(), failure);
            project.activity.pending.push(format!(
                "{ticket} needs attention: verified work is ready for review before it can be shared."
            ));
            if let Err(error) = project.queue.save(&project.state.repo_root) {
                project.queue.last_error =
                    format!("Cannot save the publication review item for {ticket}: {error}");
                return;
            }
        }
    }
}

mod start;
