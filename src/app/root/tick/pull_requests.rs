use super::*;

impl KooladeApp {
    pub(super) fn poll_pull_requests(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            if let Some(errors) = project
                .pr_refresh
                .as_ref()
                .and_then(|refresh| refresh.poll())
            {
                project.pr_refresh = None;
                project.refresh_implementations();
                if errors.is_empty()
                    && project
                        .queue
                        .last_error
                        .starts_with("Task maintenance failed for ")
                {
                    project.queue.last_error.clear();
                }
                for (ticket, error) in errors {
                    if let Some(state) = project.implementation_states.get_mut(&ticket) {
                        if state.status == ImplementationStatus::Completed {
                            state.cleanup.error = Some(error.clone());
                        } else {
                            state.pr_check_error = Some(error.clone());
                        }
                    }
                    project.queue.last_error =
                        format!("Task maintenance failed for {ticket}: {error}");
                }
            }
            if project.pr_refresh.is_none()
                && project
                    .last_pr_refresh
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
            {
                let mut states = project
                    .implementation_states
                    .iter()
                    .filter(|(_, state)| {
                        (state.status != ImplementationStatus::Completed
                            && state.pr_url.is_some()
                            && state.pr_state != Some(PullRequestState::Merged))
                            || (state.status == ImplementationStatus::Completed
                                && state.cleanup.completed_at.is_none())
                    })
                    .collect::<Vec<_>>();
                states.sort_by_key(|(_, state)| {
                    if state.status == ImplementationStatus::Completed {
                        &state.cleanup.attempted_at
                    } else {
                        &state.pr_check_attempted_at
                    }
                });
                let tickets = states
                    .into_iter()
                    .map(|(ticket, _)| ticket.clone())
                    .collect::<Vec<_>>();
                if !tickets.is_empty() {
                    project.pr_refresh =
                        Some(crate::core::implementation::PrRefresh::start_with_store(
                            project.state.planning_store.clone(),
                            project.state.repo_root.clone(),
                            tickets,
                        ));
                }
                project.last_pr_refresh = Some(Instant::now());
            }
        }
    }
}
