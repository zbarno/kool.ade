use super::*;

mod retry_claims;

type CompletedTaskOutcome = (String, String, String, TurnOutcome);
type PolledTurnEvents = (Option<TurnOutcome>, Vec<CompletedTaskOutcome>);

#[cfg(test)]
#[path = "turns/tests.rs"]
mod tests;

impl KooladeApp {
    pub(super) fn poll_turn_events(&mut self) -> PolledTurnEvents {
        // Phase 1: drain pending turn events (borrows `self.screen` only).
        let mut outcome: Option<TurnOutcome> = None;
        let mut task_outcomes = Vec::new();
        if let Screen::Connected(project) = &mut self.screen {
            project.bind_task_conversation_identities();
            project.task_chats.ensure_loaded(&project.chat_slug);
            retry_claims::renew(project);
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
                                .update(*progress);
                            project
                                .activity
                                .conversations
                                .entry(key.clone())
                                .or_default()
                                .update(Default::default());
                        }
                        Some(TurnEvt::Done(outcome)) => {
                            task_outcomes.push((
                                key.clone(),
                                ctrl.author_run_id.clone(),
                                ctrl.author_session_id.clone(),
                                *outcome,
                            ));
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
                            project.live_progress.update(*progress);
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
        task_outcomes: Vec<CompletedTaskOutcome>,
    ) {
        // Phase 2: apply a completed turn. The project is detached first so
        // the `&mut self` work (caches, toasts) cannot alias `self.screen`.
        if let Some(o) = outcome {
            let applied = matches!(&o, TurnOutcome::Applied { .. });
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                let requested_action = self.adopt_turn(&mut project, o, None, None, false);
                let pending_planner = if applied {
                    project.activity.pending_clarification_planner.take()
                } else {
                    project.activity.pending_clarification_planner = None;
                    None
                };
                if let Some((feature_uid, plan_revision)) = pending_planner {
                    self.screen = Screen::Connected(project);
                    self.continue_feature_generation(applied);
                    self.start_clarification_planner(&feature_uid, plan_revision);
                } else {
                    let automatic_planner = if applied {
                        match super::super::clarification::planner_for_changed_active_feature(
                            &project,
                        ) {
                            Ok(planner) => planner,
                            Err(error) => {
                                self.toasts.warning(format!(
                                    "The changed feature could not be scheduled for clarification planning: {error}"
                                ));
                                None
                            }
                        }
                    } else {
                        None
                    };
                    self.screen = Screen::Connected(project);
                    if let Some((feature_uid, plan_revision)) = automatic_planner {
                        self.start_clarification_planner(&feature_uid, plan_revision);
                    } else {
                        self.continue_feature_generation(applied);
                    }
                }
                if let Some(action) = requested_action {
                    requested_action::dispatch(self, action);
                }
            }
        }
        for (key, run_id, session_id, outcome) in task_outcomes {
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                let expected_user = project.task_turn_user_message_ids.remove(&key);
                let superseded = if let Some(expected) = expected_user.as_ref() {
                    project.bind_task_conversation_identities();
                    project.task_chats.refresh_now(&project.chat_slug);
                    project
                        .activity
                        .pending
                        .extend(project.task_chats.take_updates());
                    project
                        .task_chats
                        .messages
                        .get(&key)
                        .and_then(|messages| {
                            messages.iter().rev().find(|message| {
                                message.role == ChatRole::User && !message.clarification
                            })
                        })
                        .is_none_or(|current| &current.id != expected)
                } else {
                    false
                };
                project.task_turn_reply_claims.remove(&key);
                project.task_turns.remove(&key);
                project.task_live.remove(&key);
                let committed = matches!(&outcome, TurnOutcome::Applied { .. });
                if superseded && !committed {
                    project.activity.pending.push(format!(
                        "Discarded an older response for {key} because a newer task reply was saved. The current reply remains in the same task conversation."
                    ));
                    self.screen = Screen::Connected(project);
                    continue;
                }
                // Adoption routes messages to this conversation; preserve the
                // independent Main Chat worker and its live output.
                let main = project.active_turn.take();
                let live = std::mem::take(&mut project.live_progress);
                project.task_chats.active = Some(key.clone());
                let _ = self.adopt_turn(
                    &mut project,
                    outcome,
                    Some(&key),
                    Some((run_id, session_id)),
                    superseded,
                );
                project.active_turn = main;
                project.live_progress = live;
                self.screen = Screen::Connected(project);
            }
        }
    }
}
