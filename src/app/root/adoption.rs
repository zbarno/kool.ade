use super::*;

impl KooladeApp {
    /// Derived caches read by other layers through `Surface`.
    pub(super) fn refresh_derived(&mut self, project: &Project) {
        (self.cached_user, self.synth) = Self::derive_caches(project);
    }

    /// Pure derivation of the display caches, kept separated from `&mut
    /// self` so `tick` can compute them without double-borrowing.
    ///
    /// The cached routing identity is THE SEATED OPERATOR (FR-13), so the
    /// panel highlight, the chat eligibility display, and the turn pipeline
    /// all judge one and the same identity.
    pub(super) fn derive_caches(project: &Project) -> (CurrentUser, Vec<OpenItem>) {
        (
            project.state.effective_user(),
            crate::core::ownership::synthesize_for_state(&project.state),
        )
    }

    pub(super) fn adopt_turn(
        &mut self,
        project: &mut Project,
        outcome: TurnOutcome,
    ) -> Option<crate::harness::RequestedAction> {
        if let TurnOutcome::HarnessFailed { error, .. } = &outcome
            && let Some(issue) = crate::app::setup_attention::from_harness_failure(&error.detail())
        {
            self.setup_attention = Some(issue);
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
            && let Ok(work) = crate::core::planning_work::load(&state.repo_root)
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
                if let Some(work) = project.planning_work.iter_mut().find(|w| {
                    w.key == key
                        && !project
                            .cancelled_work
                            .contains(&crate::persistence::cancelled_work::planning_id(&w.uid))
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
                match project.save_planning_work() {
                    Ok(()) => {}
                    Err(error) => {
                        self.toasts
                            .danger(format!("Cannot save planning board: {error}"));
                    }
                }
            }
        }
        project.activity.ensure_overall();
        project
            .activity
            .overall
            .as_mut()
            .unwrap()
            .update(Default::default());
        let activity_key = project
            .task_chats
            .active
            .clone()
            .unwrap_or_else(|| "__main".into());
        project
            .activity
            .conversations
            .entry(activity_key)
            .or_default()
            .update(Default::default());
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
                project.state =
                    crate::core::state::PlannerState::load(&state.repo_root).unwrap_or(*state);
                project.task_documents = crate::artifacts::task_docs::load_board(
                    &project.state.repo_root,
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
                project.remember_turn_chat(chat);
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
            TurnOutcome::Rejected {
                problems,
                final_text,
                ..
            } => {
                let mut chat = vec![ChatMessage::new(
                    ChatRole::System,
                    format!(
                        "⚠ Turn rejected — nothing was written.\n{}",
                        problems.join("\n")
                    ),
                    None,
                )];
                chat.push(ChatMessage::new(
                    ChatRole::Agent,
                    if final_text.trim().is_empty() {
                        "(no legible reply — try again)".to_string()
                    } else {
                        final_text
                    },
                    None,
                ));
                project.remember_turn_chat(chat);
                self.toasts.danger(format!(
                    "Rejected: {}",
                    problems.first().map(String::as_str).unwrap_or("")
                ));
            }
            TurnOutcome::HarnessFailed { error, .. } => {
                project.remember_turn_chat(vec![ChatMessage::new(
                    ChatRole::System,
                    match &error {
                        crate::error::AppError::InvalidResponse { .. } => {
                            format!("Task generation needs attention: {}", error.detail())
                        }
                        _ => format!("Planning stopped: {}", error.headline()),
                    },
                    None,
                )]);
                self.toasts.danger(error.headline());
            }
        }
        project.task_chats.active = None;
        requested_action
    }
}
