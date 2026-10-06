use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn approve_publication(&mut self, ticket: &str) {
        let eligible = matches!(&self.screen, Screen::Connected(project)
            if !project.active_implementations.contains_key(ticket)
                && project.implementation_states.get(ticket).is_some_and(|state|
                    state.pr_url.is_none()
                        && matches!(state.status,
                            ImplementationStatus::AwaitingApproval
                                | ImplementationStatus::ReadyToPublish)));
        if eligible {
            if let Screen::Connected(project) = &mut self.screen {
                project.bind_task_conversation_identities();
                project.task_chats.ensure_loaded(&project.chat_slug);
                const DECISION: &str =
                    "I approve creating a pull request for the verified implementation.";
                if !project
                    .task_chats
                    .messages
                    .get(ticket)
                    .is_some_and(|messages| messages.iter().any(|message| message.text == DECISION))
                    && let Err(error) = project.task_chats.append(
                        &project.chat_slug,
                        ticket,
                        vec![ChatMessage::new(
                            ChatRole::User,
                            DECISION,
                            Some(ticket.to_owned()),
                        )],
                    )
                {
                    self.toasts.warning(error);
                    return;
                }
            }
            self.start_implementation(ticket.to_owned(), true);
        }
    }

    pub(in crate::app::root) fn start_implementation(&mut self, ticket: String, manual: bool) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_capabilities(ticket, manual, capabilities);
    }

    pub(in crate::app::root) fn start_implementation_with_capabilities(
        &mut self,
        ticket: String,
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) {
        if matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || p.active_implementations.contains_key(&ticket) || p.active_implementations.len() >= p.queue.max_parallel.clamp(1, 8))
        {
            return;
        }
        if !capabilities.implementation {
            let message = capabilities.implementation_unavailable_message().to_owned();
            if let Screen::Connected(project) = &mut self.screen {
                project.queue.last_error = message.clone();
            }
            self.toasts.warning(message);
            return;
        }
        if let Screen::Connected(p) = &mut self.screen {
            if p.task_cancelled(&ticket) {
                return;
            }
            if p.implementation_states.get(&ticket).is_some_and(|state| {
                state.pr_url.is_some() || state.status == ImplementationStatus::Completed
            }) {
                return;
            }
            if !p
                .task_documents
                .iter()
                .any(|d| d.path == ticket && !d.path.ends_with("/README.md"))
            {
                return;
            }
            if let Some(doc) = p.task_documents.iter().find(|d| d.path == ticket)
                && let Some(id) = doc
                    .text
                    .lines()
                    .find_map(|line| line.strip_prefix("Feature ID: "))
                && !crate::core::workflow::feature_approved(
                    &p.state.repo_root,
                    &p.state.workflow,
                    id,
                )
            {
                p.queue.last_error = format!("{id} needs explicit approval before implementation");
                return;
            }
            let target_repo =
                match crate::core::implementation::target_repository(&p.state.repo_root, &ticket) {
                    Ok(target) => target,
                    Err(error) => {
                        let message = format!("Cannot start {ticket}: {error}");
                        p.queue.last_error = message.clone();
                        p.queue.blocked.insert(
                            ticket.clone(),
                            crate::core::implementation::Failure::other(message.clone()),
                        );
                        self.toasts.danger(message);
                        return;
                    }
                };
            if let Err(error) = crate::core::implementation_queue::ticket_readiness(
                &p.task_documents,
                &p.implementation_states,
                &ticket,
            ) {
                p.queue.last_error = error;
                return;
            }
            let running = p
                .active_implementations
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if let Some(reason) = crate::core::implementation_queue::active_scope_conflict(
                &p.task_documents,
                &ticket,
                &running,
            ) {
                p.queue.last_error = reason;
                return;
            }
            let explicit_publish = manual
                && p.implementation_states.get(&ticket).is_some_and(|state| {
                    matches!(
                        state.status,
                        ImplementationStatus::ReadyToPublish
                            | ImplementationStatus::AwaitingApproval
                    )
                });
            let requested_changes = p
                .implementation_states
                .get(&ticket)
                .is_some_and(|state| state.status == ImplementationStatus::ChangesRequested);
            let publication_mode = if explicit_publish {
                crate::core::implementation::PublicationMode::CreatePullRequest
            } else if requested_changes {
                crate::core::implementation::PublicationMode::HoldForReview
            } else if p.queue.auto_publish {
                crate::core::implementation::PublicationMode::AutoPublish
            } else {
                crate::core::implementation::PublicationMode::HoldForReview
            };
            let require_independent_checks = publication_mode
                == crate::core::implementation::PublicationMode::AutoPublish
                || p.queue.require_independent_checks;
            if p.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&p.state.repo_root) {
                    Ok(lock) => p.queue_lock = Some(lock),
                    Err(error) => {
                        p.queue.last_error = error.to_string();
                        return;
                    }
                }
            }
            let previous_queue = p.queue.clone();
            let resuming = p.implementation_states.get(&ticket).is_some_and(|state| {
                matches!(
                    state.status,
                    ImplementationStatus::Blocked | ImplementationStatus::Interrupted
                )
            });
            if p.queue.auto_build {
                p.queue.running = true;
                p.queue.recovery_paused = false;
            }
            if manual {
                p.queue.recovery_attempts.remove(&ticket);
                p.queue.recovery_paused = false;
            }
            p.queue.in_flight.insert(ticket.clone());
            p.queue.blocked.remove(&ticket);
            p.queue.last_error.clear();
            if let Err(error) = p.queue.save(&p.state.repo_root) {
                p.queue.running = false;
                p.queue.last_error = error.to_string();
                p.queue.in_flight.remove(&ticket);
                if p.active_implementations.is_empty() {
                    p.queue_lock = None;
                }
                return;
            }
            if resuming {
                match crate::core::implementation::mark_resume_started(&p.state.repo_root, &ticket)
                {
                    Ok(Some(state)) => {
                        p.implementation_states.insert(ticket.clone(), state);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        p.queue = previous_queue;
                        p.queue.last_error =
                            format!("Cannot record the resumed attempt: {error:#}");
                        if let Err(save_error) = p.queue.save(&p.state.repo_root) {
                            p.queue
                                .last_error
                                .push_str(&format!("\nCannot restore queue state: {save_error:#}"));
                        }
                        p.queue.in_flight.remove(&ticket);
                        if p.active_implementations.is_empty() {
                            p.queue_lock = None;
                        }
                        return;
                    }
                }
            }
            p.activity.pending.push(format!("Assigned task {ticket} to an implementation worker. Verification and integration are managed by the queue."));
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!(
                    "Assigned {ticket}; the worker will verify and {}.",
                    if explicit_publish {
                        "share the already verified changes for review"
                    } else if p.queue.auto_publish {
                        "publish verified changes automatically"
                    } else if p.queue.auto_build {
                        "verify the work and keep it local until you choose to publish"
                    } else {
                        "verify the work; no remote publication happens automatically"
                    }
                ),
                None,
            )]);
            p.activity.tasks.entry(ticket.clone()).or_default().activity =
                Some("Starting implementation…".into());
            p.activity
                .tasks
                .entry(ticket.clone())
                .or_default()
                .telemetry = crate::harness::ActivityTelemetry {
                started_ms: Some(chrono::Utc::now().timestamp_millis()),
                ..Default::default()
            };
            p.activity.mark_ticket_dirty(&ticket);
            p.bind_task_conversation_identities();
            p.task_chats.ensure_loaded(&p.chat_slug);
            let user_name = p.state.effective_user().name;
            let user_context =
                p.task_chats.messages.get(&ticket).and_then(|messages| {
                    implementation_decision::latest_context(messages, &user_name)
                });
            p.active_implementations.insert(
                ticket.clone(),
                crate::core::implementation::Controller::start_project_with_policy(
                    p.state.repo_root.clone(),
                    target_repo,
                    ticket,
                    publication_mode,
                    require_independent_checks,
                    user_context,
                    super::super::configured_harness(&mut self.task_harness),
                ),
            );
        }
    }

    pub(in crate::app::root) fn request_publication_changes(&mut self, ticket: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_implementations.contains_key(ticket) {
            return;
        }
        let Some(mut state) = project.implementation_states.get(ticket).cloned() else {
            return;
        };
        if !matches!(
            state.status,
            ImplementationStatus::AwaitingApproval | ImplementationStatus::ReadyToPublish
        ) || state.pr_url.is_some()
        {
            return;
        }
        state.status = ImplementationStatus::ChangesRequested;
        state.detail = "Changes requested before PR approval. Describe the requested changes in this task's conversation to resume implementation.".into();
        let Ok(dir) = crate::core::implementation::state_dir_for_task(
            &project.state.repo_root,
            ticket,
            state.task_uid.as_deref(),
        ) else {
            return;
        };
        if let Err(error) = crate::core::implementation::save(&dir, &state) {
            project.queue.last_error = format!("Cannot save the review decision: {error:#}");
            return;
        }
        project
            .implementation_states
            .insert(ticket.to_owned(), state);
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        project.task_chats.remember_response(
            &project.chat_slug,
            ticket,
            vec![crate::domain::ChatMessage::new(
                crate::domain::ChatRole::User,
                "I am requesting changes before approving a pull request.",
                Some(ticket.to_owned()),
            )],
        );
    }
}
