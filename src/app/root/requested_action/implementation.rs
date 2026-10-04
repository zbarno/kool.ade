use super::*;

mod publish;

pub(super) fn publish(app: &mut KooladeApp, target: Option<String>) {
    publish::publish(app, target);
}

pub(super) fn start_from_button(app: &mut KooladeApp) {
    let feature_id = match &app.screen {
        Screen::Connected(project) => {
            let active = project.active_implementations.keys().cloned().collect();
            let ticket = crate::core::implementation_queue::next_ready_ticket(
                &project.task_documents,
                &project.implementation_states,
                &active,
            )
            .ok()
            .flatten();
            ticket
                .and_then(|ticket| {
                    project
                        .task_documents
                        .iter()
                        .find(|document| document.path == ticket)
                        .and_then(|document| {
                            document
                                .text
                                .lines()
                                .find_map(|line| line.strip_prefix("Feature ID: "))
                                .map(str::to_owned)
                        })
                })
                .or_else(|| {
                    project
                        .state
                        .active_feature
                        .as_ref()
                        .map(|(id, _)| id.clone())
                })
        }
        _ => None,
    };
    if let Some(feature_id) = feature_id {
        let needs_approval = matches!(&app.screen, Screen::Connected(project)
        if !crate::core::workflow::feature_approved(
            &project.state.repo_root,
            &project.state.workflow,
            &feature_id,
        ));
        if needs_approval {
            app.approve_feature_only(&feature_id);
        }
    }
    start(app, None, false);
}

pub(super) fn start(app: &mut KooladeApp, target: Option<String>, resume: bool) {
    let Screen::Connected(project) = &app.screen else {
        return;
    };
    if !has_current_task_batch(project) {
        action_feedback(
            app,
            "There is no task batch for the current approved change. Generate and review its task stories first.",
        );
        return;
    }
    let active = project.active_implementations.keys().cloned().collect();
    let selected = if let Some(target) = target.as_deref() {
        project
            .task_documents
            .iter()
            .find(|document| target::task_matches(document, target))
            .map(|document| document.path.clone())
            .ok_or_else(|| format!("No current task matches ID {target}."))
    } else if resume {
        let mut candidates = project
            .task_documents
            .iter()
            .filter(|document| !document.path.ends_with("/README.md"))
            .filter(|document| {
                project.queue.blocked.contains_key(&document.path)
                    || project
                        .implementation_states
                        .get(&document.path)
                        .is_some_and(|state| {
                            matches!(
                                state.status,
                                ImplementationStatus::Blocked | ImplementationStatus::Interrupted
                            )
                        })
            })
            .map(|document| document.path.clone())
            .collect::<Vec<_>>();
        candidates.sort();
        match candidates.len() {
            1 => Ok(candidates.pop().unwrap()),
            0 => Err("No blocked or interrupted task is ready to resume. Start an eligible task from the board instead.".into()),
            _ => Err("More than one task needs resuming. Open the task you want and use its Resume action.".into()),
        }
    } else {
        crate::core::implementation_queue::next_ready_ticket(
            &project.task_documents,
            &project.implementation_states,
            &active,
        )
        .map_err(|reason| format!("No task can start yet: {reason}"))
        .and_then(|ticket| {
            ticket.ok_or_else(|| "No implementation task is ready to start.".to_owned())
        })
    };
    let ticket = match selected {
        Ok(ticket) => ticket,
        Err(error) => {
            action_feedback(app, &error);
            return;
        }
    };
    if resume {
        let recoverable = project.queue.blocked.contains_key(&ticket)
            || project
                .implementation_states
                .get(&ticket)
                .is_some_and(|state| {
                    matches!(
                        state.status,
                        ImplementationStatus::Blocked | ImplementationStatus::Interrupted
                    )
                });
        if !recoverable {
            action_feedback(
                app,
                "That task is not blocked or interrupted, so Resume would not be appropriate. Use Start implementation for a task that is ready to begin.",
            );
            return;
        }
    } else if project.queue.blocked.contains_key(&ticket)
        || project
            .implementation_states
            .get(&ticket)
            .is_some_and(|state| {
                matches!(
                    state.status,
                    ImplementationStatus::Blocked | ImplementationStatus::Interrupted
                )
            })
    {
        let title = target::task_title(&project.task_documents, &ticket);
        let detail = project
            .queue
            .blocked
            .get(&ticket)
            .map(|failure| failure.message.as_str())
            .or_else(|| {
                project
                    .implementation_states
                    .get(&ticket)
                    .map(|state| state.detail.as_str())
            })
            .filter(|detail| !detail.trim().is_empty())
            .unwrap_or("the worker stopped before finishing");
        action_feedback(
            app,
            &format!(
                "{title} is blocked: {}. Open its Needs attention details, then use Resume implementation to retry the preserved work.",
                crate::core::context_build::clip(detail, 320)
            ),
        );
        return;
    }
    if let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    {
        if let Some(feature_id) = document
            .text
            .lines()
            .find_map(|line| line.strip_prefix("Feature ID: "))
            && !crate::core::workflow::feature_approved(
                &project.state.repo_root,
                &project.state.workflow,
                feature_id,
            )
        {
            action_feedback(
                app,
                &format!(
                    "{feature_id} is not approved for its current specification. Approve the change before starting or resuming this task."
                ),
            );
            return;
        }
        if let Err(reason) = crate::core::implementation_queue::ticket_readiness(
            &project.task_documents,
            &project.implementation_states,
            &ticket,
        ) {
            action_feedback(
                app,
                &format!("{} cannot start yet: {reason}", document.title),
            );
            return;
        }
    }
    app.start_implementation(ticket.clone(), resume);
    if let Screen::Connected(project) = &app.screen
        && !project.active_implementations.contains_key(&ticket)
    {
        let reason = if !project.queue.last_error.is_empty() {
            project.queue.last_error.clone()
        } else {
            format!("{} did not start; review its current task state.", ticket)
        };
        action_feedback(app, &reason);
    }
}

pub(super) fn pause(app: &mut KooladeApp, target: Option<String>) {
    let Screen::Connected(project) = &app.screen else {
        return;
    };
    if let Some(target) = target.as_deref() {
        let ticket = project
            .task_documents
            .iter()
            .find(|document| target::task_matches(document, target))
            .map(|document| document.path.clone());
        let Some(ticket) = ticket else {
            action_feedback(app, &format!("No current task matches ID {target}."));
            return;
        };
        if !project.active_implementations.contains_key(&ticket) {
            action_feedback(
                app,
                "That task is not currently running, so there is nothing to pause.",
            );
            return;
        }
        let title = target::task_title(&project.task_documents, &ticket);
        app.cancel_task_for(&ticket);
        action_feedback(
            app,
            &format!("Paused {title}. Its work is preserved for resume."),
        );
    } else if project.queue.running || !project.active_implementations.is_empty() {
        app.cancel_task();
        action_feedback(
            app,
            "Paused the implementation queue. Active task work is preserved for resume.",
        );
    } else {
        action_feedback(app, "No implementation task or queue is currently running.");
    }
}

use super::action_feedback;
