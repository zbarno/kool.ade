use super::*;
mod actions;
mod claim;
mod review;

impl KooladeApp {
    pub(in crate::app::root) fn take_over_stale_task_claim(&mut self, ticket: String) {
        let session = match &self.screen {
            Screen::Connected(project) => project
                .queue
                .stale_claim
                .as_ref()
                .filter(|(claimed_ticket, _)| claimed_ticket == &ticket)
                .map(|(_, record)| record.session_id.clone()),
            Screen::Welcome => None,
        };
        let Some(session) = session else { return };
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(ticket, true, capabilities, Some(session), false);
    }

    fn start_implementation_with_claim_mode(
        &mut self,
        ticket: String,
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
        stale_session: Option<String>,
        allow_offline: bool,
    ) {
        if matches!(&self.screen, Screen::Connected(p) if p.active_implementations.contains_key(&ticket) || p.active_implementations.len() >= p.queue.max_parallel.clamp(1, 8))
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
            let claim_request = match claim::prepare(
                &p.state.repo_root,
                &p.task_documents,
                &ticket,
                stale_session.as_deref(),
                allow_offline && manual && stale_session.is_none(),
            ) {
                Ok(request) => request,
                Err(error) => {
                    p.queue.last_error = error.to_string();
                    self.toasts.warning(error.to_string());
                    return;
                }
            };
            p.queue.stale_claim = None;
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
            // Explicit offline execution must never publish remotely. A fresh
            // coordinated run is required before creating a PR or pushing.
            let publication_mode = if allow_offline {
                crate::core::implementation::PublicationMode::HoldForReview
            } else if explicit_publish {
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
            if allow_offline {
                p.activity.pending.push(format!(
                    "{ticket}: operator explicitly chose local-only execution; automatic publication is disabled until a new remote claim is acquired."
                ));
            }
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!(
                    "Assigned {ticket}; the worker will verify and {}.",
                    if allow_offline {
                        "keep all work local without a shared claim; publishing requires a fresh coordinated attempt"
                    } else if explicit_publish {
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
            let task_routes = p
                .task_documents
                .iter()
                .find(|doc| doc.path == ticket)
                .and_then(|doc| doc.metadata.as_ref())
                .map(|metadata| metadata.routing_overrides.clone())
                .unwrap_or_default();
            let harness = super::super::configured_harness_for_task(
                &mut self.task_harness,
                Some(crate::persistence::harness_settings::IMPLEMENTATION),
                &task_routes,
            );
            let route_label = harness.label();
            if let Some(progress) = p.activity.tasks.get_mut(&ticket) {
                progress.selected_route = Some(route_label.clone());
            }
            p.active_implementations.insert(
                ticket.clone(),
                crate::core::implementation::Controller::start_project_with_policy_and_claim_request(
                    p.state.repo_root.clone(),
                    target_repo,
                    ticket,
                    crate::core::implementation::StartPolicy {
                        publication_mode,
                        require_independent_checks,
                    },
                    user_context,
                    harness,
                    Some(claim_request),
                ),
            );
        }
    }
}
