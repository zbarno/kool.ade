use super::*;
use chat::remember_turn_messages;

impl KooladeApp {
    pub(in crate::app::root) fn adopt_turn(
        &mut self,
        project: &mut Project,
        outcome: TurnOutcome,
        task_conversation: Option<&str>,
        task_author: Option<(String, String)>,
        superseded_task_reply: bool,
    ) -> Option<crate::harness::RequestedAction> {
        if let Some(key) = task_conversation {
            project.task_chats.active = Some(key.to_owned());
        }
        if let TurnOutcome::Discussion {
            final_text,
            clarification,
            ..
        } = &outcome
        {
            let mut response = ChatMessage::new(ChatRole::Agent, final_text, None);
            response.clarification = *clarification;
            remember_turn_messages(project, None, vec![response]);
            project.task_chats.active = None;
            return None;
        }
        let plan_disposition_conversation = task_conversation
            .or(project.active_planning_work.as_deref())
            .filter(|key| {
                project.planning_work.iter().any(|work| {
                    work.key == *key
                        && work.kind == crate::core::planning_work::WorkKind::PlanReviewDisposition
                })
            })
            .map(str::to_owned);
        let author_provenance = task_author.or_else(|| {
            project
                .active_turn
                .as_ref()
                .map(|turn| (turn.author_run_id.clone(), turn.author_session_id.clone()))
        });
        let plan_feedback_refresh =
            plan_feedback::capture(project, plan_disposition_conversation.as_deref());
        if let TurnOutcome::HarnessFailed { error, .. } = &outcome
            && let Some(issue) = crate::app::setup_attention::from_harness_failure(&error.detail())
        {
            if let Some(existing) = self
                .setup_attention
                .iter_mut()
                .find(|existing| existing.id == issue.id)
            {
                *existing = issue;
            } else {
                self.setup_attention.push(issue);
            }
        }
        // A selected task chat can change while a planning turn is running.
        // Keep the turn bound to the planning task that started it.
        if task_conversation.is_none() && project.active_planning_work.is_some() {
            project.task_chats.active = None;
        }
        if let TurnOutcome::Applied {
            state, normalized, ..
        } = &outcome
            && !normalized.planning_tasks.is_empty()
            && let Ok(work) = crate::core::planning_work::load(&state.planning_store)
        {
            project.planning_work = work;
        }
        let cancelled_planning_turn = task_conversation.is_none()
            && project
                .active_planning_work
                .as_deref()
                .and_then(|key| project.planning_work.iter().find(|work| work.key == key))
                .is_some_and(|work| {
                    project
                        .cancelled_work
                        .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
                });
        let requested_action = if task_conversation.is_none()
            && project.task_chats.active.is_none()
            && !cancelled_planning_turn
        {
            match &outcome {
                TurnOutcome::Applied { normalized, .. } => normalized.requested_action.clone(),
                _ => None,
            }
        } else {
            None
        };
        let work_key = task_conversation
            .map(str::to_owned)
            .or_else(|| project.active_planning_work.take());
        if let Some(key) = work_key {
            let will_continue = self
                .pending_feature_generation
                .as_ref()
                .is_some_and(|(_, _, _, pending_key)| pending_key == &key);
            work::update(
                project,
                &key,
                &outcome,
                will_continue,
                superseded_task_reply,
            );
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Cannot save planning board: {error}"));
            }
        }
        if let Some(key) = task_conversation {
            project
                .activity
                .conversations
                .entry(key.to_owned())
                .or_default()
                .update(Default::default());
        } else {
            self.reset_turn_activity(project);
        }
        project.active_turn = None;
        project.live_progress = crate::harness::LiveProgress::default();
        match outcome {
            TurnOutcome::Applied {
                state,
                receipt,
                normalized,
                commit_result,
                ..
            } => {
                let previous_batches = project.state.workflow.task_batches.len();
                let previous_state = project.state.clone();
                // Another worker may have committed since this outcome was
                // queued. Adopt current disk truth, never an older snapshot.
                project.state = crate::core::state::PlannerState::load_with_store(
                    &state.repo_root,
                    &state.planning_store,
                )
                .unwrap_or(*state);
                if let Some(key) = task_conversation {
                    let excluded = project
                        .planning_work
                        .iter()
                        .filter(|work| {
                            project.task_turns.contains_key(&work.key)
                                || project.task_chats.has_failed_task_reply(&work.key)
                                || project.task_chats.has_pending_user_reply(&work.key)
                                || project.cancelled_work.contains(
                                    &crate::persistence::cancelled_work::planning_id(&work.uid),
                                )
                        })
                        .map(|work| work.key.clone())
                        .collect();
                    if crate::core::planning_work::reconcile_answered_child(
                        &previous_state,
                        &project.state,
                        key,
                        &mut project.planning_work,
                        &excluded,
                    ) && let Err(error) = project.save_planning_work()
                    {
                        self.toasts
                            .danger(format!("Cannot save refreshed planning parent: {error}"));
                    }
                }
                self.record_plan_author_provenance(
                    project,
                    &normalized.document_updates,
                    author_provenance,
                );
                plan_feedback::schedule(
                    project,
                    plan_feedback_refresh.as_ref(),
                    &normalized.document_updates,
                );
                project.task_documents = crate::artifacts::task_docs::load_board(
                    &project.state.planning_store,
                    &project.state.workflow,
                );
                if project.task_chats.active.is_none() {
                    project.next_question_id = normalized.next_question_id.clone();
                } else if project
                    .next_question_id
                    .as_ref()
                    .is_some_and(|id| !project.state.items.iter().any(|item| &item.id == id))
                {
                    project.next_question_id = None;
                }
                let mut chat = if superseded_task_reply && task_conversation.is_some() {
                    vec![ChatMessage::new(
                        ChatRole::System,
                        "An earlier saved reply was applied before the newer reply arrived. The newer reply remains saved for processing.",
                        task_conversation.map(str::to_owned),
                    )]
                } else {
                    vec![ChatMessage::new(
                        ChatRole::Agent,
                        normalized.assistant_message,
                        normalized.next_question_id.clone(),
                    )]
                };
                if !receipt.synthesized_open_items.is_empty() {
                    chat.push(ChatMessage::new(
                        ChatRole::System,
                        format!(
                            "Raised ownership gap(s): {}",
                            receipt.synthesized_open_items.join(", ")
                        ),
                        None,
                    ));
                }
                chat.extend(
                    normalized
                        .warnings
                        .iter()
                        .map(|w| ChatMessage::new(ChatRole::System, w.clone(), None)),
                );
                if project.state.workflow.task_batches.len() > previous_batches
                    && let Some(batch) = project.state.workflow.task_batches.last()
                {
                    chat.push(ChatMessage::new(ChatRole::System, format!("Created {} detailed task stories in {}. Open the Task stories tab to review them.", batch.count, batch.directory), None));
                }
                remember_turn_messages(project, plan_disposition_conversation.as_deref(), chat);
                if let Some(key) = project.task_chats.active.clone() {
                    project.remember_turn_chat(vec![ChatMessage::new(
                        ChatRole::System,
                        if superseded_task_reply {
                            "An earlier task reply was applied. The newer reply remains saved for processing.".into()
                        } else {
                            format!(
                                "Task reply applied to planning artifacts. {}",
                                if project
                                    .state
                                    .resolved_items
                                    .iter()
                                    .any(|i| i.conversation_key() == key)
                                {
                                    "This question is resolved."
                                } else {
                                    "See the current task for any remaining questions."
                                }
                            )
                        },
                        Some(key),
                    )]);
                }
                project.refresh_git();
                self.refresh_derived(project);
                match &commit_result {
                    Ok(sha) => self.toasts.success(format!(
                        "Checkpoint {} · {}",
                        sha.chars().take(7).collect::<String>(),
                        receipt.commit_message
                    )),
                    Err(e) => self
                        .toasts
                        .warning(format!("Applied; git checkpoint failed: {}", e.headline())),
                }
            }
            TurnOutcome::Discussion { .. } => {
                unreachable!("discussion handled before state adoption")
            }
            TurnOutcome::Rejected {
                problems,
                final_text,
                ..
            } => self.adopt_rejected_turn(
                project,
                problems,
                final_text,
                plan_disposition_conversation.as_deref(),
            ),
            TurnOutcome::HarnessFailed { error, .. } => {
                self.adopt_failed_turn(project, error, plan_disposition_conversation.as_deref())
            }
        }
        project.task_chats.active = None;
        requested_action
    }
}
