use super::*;

impl KooladeApp {
    pub(super) fn poll_display_refresh(&mut self, ctx: &egui::Context) {
        // Poll only: repository subprocesses and reads must not stall input frames.
        if self
            .display_refresh
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            if let Ok(result) = self.display_refresh.take().unwrap().join()
                && let Screen::Connected(p) = &mut self.screen
                && p.state.repo_root == result.repo
                && p.state.workflow == result.workflow
                && p.implementation_states == result.previous_implementations
            {
                p.git = result.git;
                p.task_documents = result.documents;
                p.adopt_implementations(result.implementations);
                for (ticket, activity) in result.activity {
                    p.activity.tasks.entry(ticket).or_insert(activity);
                }
                (self.cached_user, self.synth) = Self::derive_caches(p);
            }
            self.last_git_refresh = Instant::now();
        }
        if self.display_refresh.is_none()
            && self.last_git_refresh.elapsed() > Duration::from_secs(3)
            && let Screen::Connected(p) = &self.screen
        {
            let repo = p.state.repo_root.clone();
            let workflow = p.state.workflow.clone();
            let previous_implementations = p.implementation_states.clone();
            let ctx = ctx.clone();
            self.display_refresh = Some(std::thread::spawn(move || {
                let git = crate::core::gitops::snapshot(&repo);
                let documents = crate::artifacts::task_docs::load_board(&repo, &workflow);
                let implementations = crate::core::implementation::load_board_states(&repo);
                let activity = implementations
                    .keys()
                    .filter_map(|ticket| {
                        crate::core::implementation::load_activity(&repo, ticket)
                            .map(|p| (ticket.clone(), p))
                    })
                    .collect();
                ctx.request_repaint();
                DisplayRefresh {
                    repo,
                    workflow,
                    git,
                    documents,
                    implementations,
                    previous_implementations,
                    activity,
                }
            }));
        }
    }
}
