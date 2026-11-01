use super::*;

#[derive(Clone, Copy)]
enum ReplyRecovery {
    Initial,
    PlanningRetry(usize),
    Exhausted(usize),
}

impl KooladeApp {
    pub(in crate::app::root) fn retry_pending_task_reply(&mut self) {
        let candidates = match &mut self.screen {
            Screen::Connected(project) => {
                project.bind_task_conversation_identities();
                project.task_chats.ensure_loaded(&project.chat_slug);
                let has_conflict = project.task_chats.messages.keys().any(|key| {
                    project
                        .task_chats
                        .pending_planning_conflict_retries(key)
                        .is_some()
                        || project
                            .task_chats
                            .exhausted_planning_conflict_retry_claims(key)
                            .is_some()
                });
                if has_conflict {
                    project.task_chats.retry_save(&project.chat_slug);
                    project.task_chats.refresh_now(&project.chat_slug);
                }
                project
                    .task_chats
                    .messages
                    .iter()
                    .filter_map(|(key, messages)| {
                        if key.starts_with("@legacy:")
                            || key.starts_with("@unlinked:")
                            || key.starts_with("@ambiguous:")
                            || project.task_turns.contains_key(key)
                            || project.task_chats.has_ambiguous_identity(key)
                        {
                            return None;
                        }
                        let user = messages.iter().rev().find(|message| {
                            message.role == ChatRole::User && !message.clarification
                        })?;
                        let pending = project.task_chats.pending_planning_conflict_retries(key);
                        let exhausted = project
                            .task_chats
                            .exhausted_planning_conflict_retry_claims(key);
                        match (pending, exhausted) {
                            (Some(retries), _) if retries > crate::persistence::task_chats::MAX_PLANNING_CONFLICT_RETRIES => Some((
                                key.clone(),
                                user.text.clone(),
                                user.id.clone(),
                                ReplyRecovery::Exhausted(retries),
                            )),
                            (_, Some(retries)) => Some((
                                key.clone(),
                                user.text.clone(),
                                user.id.clone(),
                                ReplyRecovery::Exhausted(retries),
                            )),
                            (Some(retries), _) if retries > 0 => Some((
                                key.clone(),
                                user.text.clone(),
                                user.id.clone(),
                                ReplyRecovery::PlanningRetry(retries),
                            )),
                            (Some(_), _) => None,
                            (None, _) if project.task_chats.has_pending_user_reply(key)
                                && !project.task_chats.has_active_reply_claim(key) =>
                            {
                                Some((
                                    key.clone(),
                                    user.text.clone(),
                                    user.id.clone(),
                                    ReplyRecovery::Initial,
                                ))
                            }
                            (None, _) => None,
                        }
                    })
                    .collect::<Vec<_>>()
            }
            Screen::Welcome => Vec::new(),
        };

        for (key, text, user_id, recovery) in candidates {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            if let ReplyRecovery::Exhausted(retries) = recovery {
                match project.task_chats.mark_planning_conflict_retry_exhausted(
                    &project.chat_slug,
                    &key,
                    &user_id,
                    retries,
                ) {
                    Ok(true) => {
                        if let Some(work) = project
                            .planning_work
                            .iter_mut()
                            .find(|work| work.key == key)
                        {
                            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
                            work.detail = "The internal planning-conflict recovery limit was reached. The original answer remains saved for internal recovery; do not ask the user to repeat it.".into();
                            if let Err(error) = project.save_planning_work() {
                                project.queue.last_error = format!(
                                    "Exhausted saved answer was preserved, but its recovery status could not be saved: {error}"
                                );
                            }
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        project.queue.last_error =
                            format!("Exhausted saved task reply could not be recorded: {error}");
                    }
                }
                continue;
            }
            let conflict_retries = match recovery {
                ReplyRecovery::PlanningRetry(retries) => Some(retries),
                ReplyRecovery::Initial => None,
                ReplyRecovery::Exhausted(_) => unreachable!("exhausted replies already continued"),
            };
            if let Err(error) =
                crate::core::task_conversation::prompt(&project.state, &key, &text, &[])
            {
                project.queue.last_error = format!("Saved task reply retry paused: {error}");
                continue;
            }
            let work = crate::core::planning_work::find_card(&project.state, &key);
            let work_type = work.as_ref().map(|work| work.kind.harness_route_category());
            let task_routes = work
                .as_ref()
                .map(|work| work.routing_overrides.clone())
                .unwrap_or_default();
            let harness = crate::app::root::configured_harness_for_project_task(
                &mut self.task_harness,
                work_type,
                &project.state.harness_routes.routes,
                &task_routes,
            );
            if let Some(blocker) = harness.route_blocker() {
                project.queue.last_error = format!("Saved task reply retry paused: {blocker}");
                continue;
            }
            let claim = if let Some(retries) = conflict_retries {
                project.task_chats.claim_pending_planning_conflict_retry(
                    &project.chat_slug,
                    &key,
                    &user_id,
                    retries,
                )
            } else {
                project
                    .task_chats
                    .claim_pending_task_reply(&project.chat_slug, &key, &user_id)
            };
            let claim_id = match claim {
                Ok(Some(claim_id)) => claim_id,
                Ok(None) => continue,
                Err(error) => {
                    project.queue.last_error =
                        format!("Saved task reply retry could not be claimed: {error}");
                    continue;
                }
            };
            let mut recent_chat: Vec<_> = project
                .task_chats
                .messages
                .get(&key)
                .into_iter()
                .flatten()
                .filter(|message| message.id != user_id)
                .map(|message| (format!("{:?}", message.role), message.text.clone()))
                .collect();
            if project.task_chats.has_failed_task_reply(&key) {
                recent_chat.push(("System".into(), "Recover the original saved answer from its prior pre-application failure. Repair the reported response format, preserve the submitted choice and scope, and return the full validated envelope. Omit open_items_updated for an item you resolve. Do not repeat the question or bypass authorization checks.".into()));
            }
            let purpose = if crate::core::task_conversation::is_opportunity_conversation_key(&key)
                || work
                    .as_ref()
                    .is_some_and(|work| work.kind == crate::core::planning_work::WorkKind::Question)
            {
                crate::core::workflow::TurnPurpose::Question
            } else {
                crate::core::workflow::TurnPurpose::Interview
            };
            let inputs = crate::core::turn::TurnInputs {
                state: project.state.clone(),
                user_message: text,
                recent_chat,
                purpose,
                comparison_feature: None,
            };
            let route_label = harness.label();
            project.task_turns.insert(
                key.clone(),
                std::rc::Rc::new(TurnController::start_task_reply(
                    inputs,
                    harness,
                    Some(key.clone()),
                    project.chat_slug.clone(),
                    user_id.clone(),
                    Some(claim_id.clone()),
                )),
            );
            project
                .task_turn_user_message_ids
                .insert(key.clone(), user_id.clone());
            project
                .task_turn_reply_claims
                .insert(key.clone(), (claim_id, std::time::Instant::now()));
            project.task_live.insert(
                key.clone(),
                crate::harness::LiveProgress {
                    activity: Some(if matches!(recovery, ReplyRecovery::PlanningRetry(_)) {
                        "Retrying a saved task reply after a planning revision conflict.".into()
                    } else {
                        "Resuming a saved task reply after an interrupted turn.".into()
                    }),
                    selected_route: Some(route_label),
                    ..Default::default()
                },
            );
            return;
        }
    }
}
