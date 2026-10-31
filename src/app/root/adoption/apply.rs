use super::*;
use chat::remember_turn_messages;

impl KooladeApp {
    pub(in crate::app::root) fn adopt_turn(
        &mut self,
        project: &mut Project,
        outcome: TurnOutcome,
    ) -> Option<crate::harness::RequestedAction> {
        if let TurnOutcome::Discussion { final_text, .. } = &outcome {
            remember_turn_messages(
                project,
                None,
                vec![ChatMessage::new(ChatRole::Agent, final_text, None)],
            );
            project.task_chats.active = None;
            return None;
        }
        let plan_disposition_conversation = project
            .active_planning_work
            .as_ref()
            .filter(|key| {
                project.planning_work.iter().any(|work| {
                    &work.key == *key
                        && work.kind == crate::core::planning_work::WorkKind::PlanReviewDisposition
                })
            })
            .cloned();
        let author_provenance = project
            .active_turn
            .as_ref()
            .map(|turn| (turn.author_run_id.clone(), turn.author_session_id.clone()));
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
        if project.active_planning_work.is_some() {
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
        let cancelled_planning_turn = project
            .active_planning_work
            .as_deref()
            .and_then(|key| project.planning_work.iter().find(|work| work.key == key))
            .is_some_and(|work| {
                project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
            });
        let requested_action = if project.task_chats.active.is_none() && !cancelled_planning_turn {
            match &outcome {
                TurnOutcome::Applied { normalized, .. } => normalized.requested_action.clone(),
                _ => None,
            }
        } else {
            None
        };
        {
            let work_key = project
                .task_chats
                .active
                .clone()
                .or_else(|| project.active_planning_work.take());
            if let Some(key) = work_key {
                let will_continue = self
                    .pending_feature_generation
                    .as_ref()
                    .is_some_and(|(_, _, _, pending_key)| pending_key == &key);
                if let Some(work) = project.planning_work.iter_mut().find(|work| {
                    work.key == key
                        && !project
                            .cancelled_work
                            .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
                }) {
                    match &outcome {
                        TurnOutcome::Applied { normalized, .. } => {
                            let model_offer = normalized.follow_up_task.clone().map(|offer| {
                                crate::core::planning_work::FollowUpTaskOffer {
                                    title: offer.title,
                                    description: offer.description,
                                }
                            });
                            work.follow_up_task = model_offer
                                .or_else(|| planning_work::provider_support_follow_up(work));
                            if work.kind != crate::core::planning_work::WorkKind::TaskGeneration {
                                work.feature_id = normalized
                                    .document_updates
                                    .iter()
                                    .find_map(|(id, _)| {
                                        id.strip_prefix("feature:").map(str::to_owned)
                                    })
                                    .or(work.feature_id.clone());
                            }
                            let needs_input = normalized.next_question_id.is_some()
                                || normalized.planning_tasks.iter().any(|task| {
                                    task.status
                                        == crate::core::planning_work::WorkStatus::NeedsAttention
                                })
                                || normalized.added.iter().any(|item| {
                                    matches!(
                                        item.authority,
                                        crate::domain::Authority::Human
                                            | crate::domain::Authority::Review
                                    )
                                });
                            work.status = crate::core::planning_work::completed_turn_status(
                                work.kind,
                                work.feature_id.is_some(),
                                needs_input,
                                normalized.task_batch.is_some(),
                                will_continue,
                            );
                            work.detail = if work.kind
                                == crate::core::planning_work::WorkKind::PlanReviewDisposition
                            {
                                format!(
                                    "{}\n\nPlanner update: {}",
                                    work.detail, normalized.assistant_message
                                )
                            } else if work.kind
                                == crate::core::planning_work::WorkKind::TaskGeneration
                                && normalized.task_batch.is_some()
                            {
                                "Implementation tasks were generated and added to the Kanban board."
                                    .into()
                            } else {
                                normalized.assistant_message.clone()
                            };
                        }
                        TurnOutcome::HarnessFailed { error, .. } => {
                            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
                            work.detail = format!(
                                "Planning needs attention before it can continue.\n\n{}\n\nNext action: fix the reported setup or provider issue, then retry this task.",
                                error.detail()
                            );
                        }
                        _ => {
                            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
                            work.detail = "Planning needs attention; continue this request in its conversation.".into();
                        }
                    }
                }
                crate::core::planning_work::link_feature_identities(
                    &project.state,
                    &mut project.planning_work,
                );
                if let Err(error) = project.save_planning_work() {
                    self.toasts
                        .danger(format!("Cannot save planning board: {error}"));
                }
            }
        }
        self.reset_turn_activity(project);
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
                // Another worker may have committed since this outcome was
                // queued. Adopt current disk truth, never an older snapshot.
                project.state = crate::core::state::PlannerState::load_with_store(
                    &state.repo_root,
                    &state.planning_store,
                )
                .unwrap_or(*state);
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
                let mut chat = vec![ChatMessage::new(
                    ChatRole::Agent,
                    normalized.assistant_message,
                    normalized.next_question_id.clone(),
                )];
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
                        ),
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
