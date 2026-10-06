use super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn cancel_board_work(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let mut ids = project.cancelled_work.clone();
        let mut tickets = Vec::new();
        let mut planning_key = None;
        if let Some(work) =
            crate::core::planning_work::cards(&project.state, &project.planning_work)
                .into_iter()
                .find(|work| work.key == key)
        {
            let feature_complete = work
                .feature_id
                .as_deref()
                .and_then(|id| {
                    project
                        .state
                        .active_features
                        .iter()
                        .find(|(feature_id, _)| feature_id == id)
                })
                .and_then(|(_, body)| crate::domain::ChangeMetadata::require_markdown(body).ok())
                .is_some_and(|metadata| {
                    matches!(
                        metadata.status,
                        crate::domain::ChangeStatus::Implemented
                            | crate::domain::ChangeStatus::Abandoned
                    )
                });
            if (work.status == crate::core::planning_work::WorkStatus::Done
                && (work.feature_id.is_none() || feature_complete))
                || ids.contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
            {
                return;
            }
            ids.insert(crate::persistence::cancelled_work::planning_id(&work.uid));
            planning_key = Some(work.key.clone());
            if let Some(feature_id) = work.feature_id.as_deref() {
                if let Some(feature_uid) = work.feature_uid.as_deref() {
                    ids.insert(crate::persistence::cancelled_work::planning_id(feature_uid));
                }
                for doc in project.task_documents.iter().filter(|doc| {
                    doc.text
                        .lines()
                        .any(|line| line.strip_prefix("Feature ID: ") == Some(feature_id))
                }) {
                    let completed =
                        project
                            .implementation_states
                            .get(&doc.path)
                            .is_some_and(|state| {
                                state.status
                                    == crate::core::implementation::ImplementationStatus::Completed
                                    || state.pr_state
                                        == Some(
                                            crate::core::implementation::PullRequestState::Merged,
                                        )
                            });
                    if !completed {
                        ids.insert(crate::persistence::cancelled_work::task_id(doc));
                        tickets.push(doc.path.clone());
                    }
                }
            }
        } else if let Some(doc) = project.task_documents.iter().find(|doc| doc.path == key) {
            let completed = project.implementation_states.get(key).is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Completed
                    || state.pr_state == Some(crate::core::implementation::PullRequestState::Merged)
            });
            if completed || ids.contains(&crate::persistence::cancelled_work::task_id(doc)) {
                return;
            }
            ids.insert(crate::persistence::cancelled_work::task_id(doc));
            tickets.push(key.to_owned());
        } else {
            return;
        }

        if let Err(error) = crate::persistence::cancelled_work::save(&project.state.repo_root, &ids)
        {
            self.toasts
                .danger(format!("Could not cancel work: {error}"));
            return;
        }
        project.cancelled_work = ids;
        for ticket in &tickets {
            project.queue.in_flight.remove(ticket);
            project.queue.blocked.remove(ticket);
            project.queue.recovery_attempts.remove(ticket);
            if project.queue.current_ticket.as_deref() == Some(ticket) {
                project.queue.current_ticket = None;
            }
            if let Some(controller) = project.active_implementations.get(ticket) {
                controller.request_cancel();
            }
        }
        if let Some(key) = planning_key.as_deref()
            && project.active_planning_work.as_deref() == Some(key)
            && let Some(turn) = &project.active_turn
        {
            turn.request_cancel();
        }
        if let Err(error) = project.queue.save(&project.state.repo_root) {
            self.toasts.warning(format!(
                "Work is cancelled, but its queue checkpoint could not be saved: {error}"
            ));
        } else {
            self.toasts
                .success("Work cancelled. Its history and project files are preserved.");
        }
    }
}
