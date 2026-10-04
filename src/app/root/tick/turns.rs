use super::*;

impl KooladeApp {
    pub(super) fn poll_turn_events(&mut self) -> (Option<TurnOutcome>, Vec<(String, TurnOutcome)>) {
        // Phase 1: drain pending turn events (borrows `self.screen` only).
        let mut outcome: Option<TurnOutcome> = None;
        let mut task_outcomes = Vec::new();
        if let Screen::Connected(project) = &mut self.screen {
            project.bind_task_conversation_identities();
            project.task_chats.ensure_loaded(&project.chat_slug);
            project
                .activity
                .pending
                .extend(project.task_chats.take_updates());
            project.activity.ensure_overall();
            for (key, ctrl) in &project.task_turns {
                for _ in 0..64 {
                    match ctrl.poll(Duration::ZERO) {
                        Some(TurnEvt::Progress(progress)) => {
                            project
                                .task_live
                                .entry(key.clone())
                                .or_default()
                                .update(progress);
                            project
                                .activity
                                .conversations
                                .entry(key.clone())
                                .or_default()
                                .update(Default::default());
                        }
                        Some(TurnEvt::Done(outcome)) => {
                            task_outcomes.push((key.clone(), *outcome));
                            break;
                        }
                        None => break,
                    }
                }
            }
            if let Some(ctrl) = &project.active_turn {
                for _ in 0..64 {
                    let Some(evt) = ctrl.poll(Duration::ZERO) else {
                        break;
                    };
                    match evt {
                        TurnEvt::Progress(progress) => {
                            project
                                .activity
                                .overall
                                .as_mut()
                                .unwrap()
                                .update(Default::default());
                            let key = project
                                .active_planning_work
                                .clone()
                                .or_else(|| project.task_chats.active.clone())
                                .unwrap_or_else(|| "__main".into());
                            project
                                .activity
                                .conversations
                                .entry(key)
                                .or_default()
                                .update(Default::default());
                            project.live_progress.update(progress);
                        }
                        TurnEvt::Done(o) => {
                            outcome = Some(*o);
                            break;
                        }
                    }
                }
            }
        }
        (outcome, task_outcomes)
    }

    pub(super) fn apply_completed_turns(
        &mut self,
        outcome: Option<TurnOutcome>,
        task_outcomes: Vec<(String, TurnOutcome)>,
    ) {
        // Phase 2: apply a completed turn. The project is detached first so
        // the `&mut self` work (caches, toasts) cannot alias `self.screen`.
        if let Some(o) = outcome {
            let applied = matches!(&o, TurnOutcome::Applied { .. });
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                let requested_action = self.adopt_turn(&mut project, o);
                self.screen = Screen::Connected(project);
                self.continue_feature_generation(applied);
                if let Some(action) = requested_action {
                    requested_action::dispatch(self, action);
                }
            }
        }
        for (key, outcome) in task_outcomes {
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                project.task_turns.remove(&key);
                project.task_live.remove(&key);
                // Adoption routes messages to this conversation; preserve the
                // independent Main Chat worker and its live output.
                let main = project.active_turn.take();
                let live = std::mem::take(&mut project.live_progress);
                project.task_chats.active = Some(key);
                let _ = self.adopt_turn(&mut project, outcome);
                project.active_turn = main;
                project.live_progress = live;
                self.screen = Screen::Connected(project);
            }
        }
    }
}
