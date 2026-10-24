use super::{KooladeApp, Screen};

mod self_improvement;
use self_improvement::{
    retry_evolve_revocation, retry_evolve_revocation_for_feature,
    retry_evolve_revocation_for_feature_uid,
};

impl KooladeApp {
    pub(in crate::app::root) fn cancel_board_work(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let mut ids = project.cancelled_work.clone();
        let mut tickets = Vec::new();
        let mut planning_key = None;
        let mut feature_uid_for_revocation = None;
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
            if ids.contains(&crate::persistence::cancelled_work::planning_id(&work.uid)) {
                if let Some(feature_uid) = work.feature_uid.as_deref() {
                    retry_evolve_revocation_for_feature_uid(project, feature_uid);
                }
                if let Some(feature_id) = work.feature_id.as_deref() {
                    retry_evolve_revocation_for_feature(project, feature_id);
                }
                if let Some(feature_uid) = work.feature_uid.as_deref() {
                    retry_evolve_revocation_for_feature_uid(project, feature_uid);
                }
                return;
            }
            if work.status == crate::core::planning_work::WorkStatus::Done
                && (work.feature_id.is_none() || feature_complete)
            {
                return;
            }
            ids.insert(crate::persistence::cancelled_work::planning_id(&work.uid));
            planning_key = Some(work.key.clone());
            if let Some(feature_id) = work.feature_id.as_deref() {
                if let Some(feature_uid) = work.feature_uid.as_deref() {
                    ids.insert(crate::persistence::cancelled_work::planning_id(feature_uid));
                    feature_uid_for_revocation = Some(feature_uid.to_owned());
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
            if ids.contains(&crate::persistence::cancelled_work::task_id(doc)) {
                retry_evolve_revocation(project, &[key.to_owned()]);
                return;
            }
            if completed {
                return;
            }
            ids.insert(crate::persistence::cancelled_work::task_id(doc));
            tickets.push(key.to_owned());
        } else {
            return;
        }

        let revision = match crate::persistence::cancelled_work::save_expected(
            &project.state.planning_store,
            &ids,
            &project.state.baseline_planning_revision,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                self.toasts
                    .danger(format!("Could not cancel work: {error}"));
                return;
            }
        };
        project.state.baseline_planning_revision = revision;
        project.cancelled_work = ids;
        if let Some(feature_uid) = feature_uid_for_revocation.as_deref() {
            retry_evolve_revocation_for_feature_uid(project, feature_uid);
        }
        retry_evolve_revocation(project, &tickets);
        if let Some(feature_uid) = feature_uid_for_revocation.as_deref() {
            retry_evolve_revocation_for_feature_uid(project, feature_uid);
        }
        for ticket in &tickets {
            project.queue.in_flight.remove(ticket);
            project.queue.waiting_for_capacity.remove(ticket);
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

    pub(in crate::app::root) fn pause_task_for(&mut self, ticket: &str) {
        let mut persistence_error = None;
        if let Screen::Connected(project) = &mut self.screen {
            let Some(controller) = project.active_implementations.get(ticket) else {
                return;
            };
            project.queue.pause_task_for_resume(ticket);
            if project.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&project.state.repo_root) {
                    Ok(lock) => project.queue_lock = Some(lock),
                    Err(error) => persistence_error = Some(error.to_string()),
                }
            }
            controller.request_cancel();
            if project.queue_lock.is_some()
                && let Err(error) = project.queue.save(&project.state.repo_root)
            {
                persistence_error = Some(error.to_string());
            }
            if let Some(error) = &persistence_error {
                project.queue.last_error = format!("Task pause could not be saved: {error}");
            }
        }
        if let Some(error) = persistence_error {
            self.toasts.warning(format!(
                "The task was asked to pause, but its resume checkpoint could not be saved: {error}"
            ));
        }
    }
}
