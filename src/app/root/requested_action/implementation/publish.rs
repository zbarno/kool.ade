use super::super::action_feedback;
use super::*;

pub(super) fn publish(app: &mut KooladeApp, target: Option<String>) {
    let Screen::Connected(project) = &app.screen else {
        return;
    };
    let ticket = if let Some(target) = target.as_deref() {
        match project
            .task_documents
            .iter()
            .find(|document| super::super::target::task_matches(document, target))
        {
            Some(document) => Some(document.path.clone()),
            None => {
                action_feedback(app, &format!("No current task matches ID {target}."));
                return;
            }
        }
    } else {
        let mut ready = project
            .implementation_states
            .iter()
            .filter(|(_, state)| state.status == ImplementationStatus::ReadyToPublish)
            .map(|(ticket, _)| ticket.clone())
            .collect::<Vec<_>>();
        ready.sort();
        if ready.len() == 1 { ready.pop() } else { None }
    };
    let Some(ticket) = ticket else {
        action_feedback(
            app,
            "No single verified task is ready to share. Choose a task card to see its current status and next action.",
        );
        return;
    };
    let title = super::super::target::task_title(&project.task_documents, &ticket);
    let status = project.implementation_states.get(&ticket).map(|state| {
        (
            state.status,
            state.pr_url.is_some(),
            project.active_implementations.contains_key(&ticket),
        )
    });
    match status {
        Some((_, true, _)) => action_feedback(
            app,
            &format!(
                "{title} has already been shared for review. Open its task card to see the latest status."
            ),
        ),
        Some((_, _, true)) => action_feedback(
            app,
            &format!(
                "Kool.ad/e is still working on {title}. It will finish verification before any sharing action."
            ),
        ),
        Some((ImplementationStatus::ReadyToPublish, false, false)) => {
            app.start_implementation(ticket.clone(), true);
            let started = matches!(&app.screen, Screen::Connected(project) if project.active_implementations.contains_key(&ticket));
            action_feedback(
                app,
                &if started {
                    format!(
                        "Kool.ad/e is sharing the previously verified work for {title} for review."
                    )
                } else {
                    format!(
                        "Kool.ad/e could not start sharing {title}. Open the task card for the current status and next action."
                    )
                },
            );
        }
        Some((ImplementationStatus::Completed, false, false)) => action_feedback(
            app,
            &format!("{title} is already complete; there are no unpublished changes to share."),
        ),
        Some((current, _, _)) => action_feedback(
            app,
            &format!(
                "{title} is {}. Kool.ad/e will finish implementation and verification before it can be shared.",
                current.label()
            ),
        ),
        None => action_feedback(
            app,
            &format!(
                "{title} has no saved verified work yet. Start implementation and let Kool.ad/e finish verification before sharing it."
            ),
        ),
    };
}
