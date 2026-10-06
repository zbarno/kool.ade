use super::*;

impl KooladeApp {
    pub(super) fn advance_reconciliation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let event =
            project
                .reconciliation
                .advance(&project.state, &mut self.task_harness, Instant::now());
        let mut toast = None;
        match event {
            Some(crate::app::reconciliation_lifecycle::Event::Started { feature_id }) => {
                project.activity.pending.push(format!(
                    "All tasks for {feature_id} have merged; checking actual implementation against the approved feature."
                ));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Completed {
                feature_id,
                state,
                message,
            }) => {
                project.state = state;
                project.task_documents = crate::artifacts::task_docs::load_board(
                    &project.state.repo_root,
                    &project.state.workflow,
                );
                project.refresh_git();
                project
                    .activity
                    .pending
                    .push(format!("Reconciled {feature_id}: {message}"));
                toast = Some((true, format!("Reconciled {feature_id}")));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Deferred {
                feature_id,
                error,
                state,
            }) => {
                if let Some(state) = state {
                    project.state = state;
                }
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} deferred - the project is still moving; Kool.ad/e will check again shortly. ({error})"
                ));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Failed {
                feature_id,
                error,
                state,
            }) => {
                if let Some(state) = state {
                    project.state = state;
                }
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} needs attention: {error}"
                ));
                toast = Some((false, format!("Reconciliation needs attention: {error}")));
            }
            Some(crate::app::reconciliation_lifecycle::Event::ProbeFailed {
                feature_id,
                error,
            }) => {
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} needs attention: {error}"
                ));
            }
            None => {}
        }
        if let Some((success, message)) = toast {
            if success {
                self.toasts.success(message);
            } else {
                self.toasts.warning(message);
            }
        }
    }

    pub(super) fn advance_investigation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let mut cancelled = false;
        if let Some(controller) = &project.investigation {
            let item_id = controller.item_id.clone();
            let mut finished = None;
            for _ in 0..64 {
                match controller.poll() {
                    Some(crate::core::investigation::Event::Progress(update)) => {
                        project
                            .activity
                            .overall
                            .as_mut()
                            .unwrap()
                            .update(Default::default());
                        project
                            .activity
                            .tasks
                            .entry(item_id.clone())
                            .or_default()
                            .update(*update);
                        project.activity.mark_ticket_dirty(&item_id);
                    }
                    Some(crate::core::investigation::Event::Done(result)) => {
                        cancelled = controller.cancellation_requested();
                        finished = Some(*result);
                        break;
                    }
                    None => break,
                }
            }
            if let Some(result) = finished {
                project.investigation = None;
                if let Some(progress) = project.activity.tasks.get_mut(&item_id) {
                    progress.telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
                }
                project.save_task_activity(&item_id);
                project.activity.dirty_tickets.remove(&item_id);
                if cancelled {
                    if let Ok(current) =
                        crate::core::state::PlannerState::load(&project.state.repo_root)
                    {
                        project.state = current;
                    }
                    if let Some(progress) = project.activity.tasks.get_mut(&item_id) {
                        progress.activity = Some("Paused because Auto Plan is off".into());
                    }
                    project.investigation_cooldown_until = None;
                    project.save_task_activity(&item_id);
                } else {
                    match result {
                        Ok((state, message)) => {
                            project.state = state;
                            project.investigation_cooldown_until = None;
                            project
                                .activity
                                .pending
                                .push(format!("Agent item {item_id}: {message}"));
                        }
                        Err(error) => {
                            if let Ok(current) =
                                crate::core::state::PlannerState::load(&project.state.repo_root)
                            {
                                project.state = current;
                            }
                            let error = error.to_string();
                            if error.starts_with(crate::core::reconciliation::DEFER_PREFIX) {
                                // Benign contention: keep the item open and let
                                // it retry after a quiet stretch; no alarm.
                                project.investigation_cooldown_until =
                                    Some(Instant::now() + Duration::from_secs(300));
                                project.activity.pending.push(format!(
                                "Investigation of {item_id} deferred - the project is still moving; Kool.ad/e will try again shortly."
                            ));
                            } else {
                                project.investigation_attempted.insert(item_id.clone());
                                project
                                    .activity
                                    .tasks
                                    .entry(item_id.clone())
                                    .or_default()
                                    .activity = Some(format!("Needs attention: {error}"));
                                project
                                    .activity
                                    .pending
                                    .push(format!("Agent item {item_id} needs attention: {error}"));
                            }
                            project.save_task_activity(&item_id);
                        }
                    }
                }
            }
        }
        if project.investigation.is_some()
            || !project.queue.auto_plan
            || project
                .investigation_cooldown_until
                .is_some_and(|until| until > Instant::now())
        {
            return;
        }
        let next = project
            .state
            .items
            .iter()
            .filter(|item| {
                item.authority == crate::domain::Authority::Agent
                    && item.status == crate::domain::ItemStatus::Open
                    && !project.task_turns.contains_key(item.conversation_key())
                    && !project.investigation_attempted.contains(&item.id)
            })
            .min_by_key(|item| (item.priority.rank(), &item.id));
        if let Some(item) = next {
            let item_id = item.id.clone();
            project
                .activity
                .tasks
                .entry(item_id.clone())
                .or_default()
                .telemetry
                .started_ms = Some(chrono::Utc::now().timestamp_millis());
            project
                .activity
                .tasks
                .entry(item_id.clone())
                .or_default()
                .activity = Some("Investigating repository evidence…".into());
            project.activity.mark_ticket_dirty(&item_id);
            let harness = configured_harness(&mut self.task_harness);
            project.investigation = Some(crate::core::investigation::Controller::start(
                project.state.clone(),
                item_id,
                harness,
            ));
        }
    }
}
