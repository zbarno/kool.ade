use super::*;

impl KooladeApp {
    pub(super) fn poll_implementations(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            let mut finished = Vec::new();
            let mut checklist_dirty = Vec::new();
            for (ticket, ctrl) in &project.active_implementations {
                for _ in 0..64 {
                    match ctrl.poll() {
                        Some(crate::core::implementation::Event::Progress(p)) => {
                            project
                                .activity
                                .overall
                                .as_mut()
                                .unwrap()
                                .update(Default::default());
                            let checklist_changed = {
                                let activity =
                                    project.activity.tasks.entry(ticket.clone()).or_default();
                                let revision = activity.checklist_revision;
                                activity.update(*p);
                                activity.checklist_revision != revision
                            };
                            project.activity.mark_ticket_dirty(ticket);
                            if checklist_changed {
                                checklist_dirty.push(ticket.clone());
                            }
                        }
                        Some(crate::core::implementation::Event::Done(result)) => {
                            finished.push((ticket.clone(), *result));
                            break;
                        }
                        None => break,
                    }
                }
            }
            for ticket in checklist_dirty {
                project.save_task_activity(&ticket);
            }
            for (ticket, result) in finished {
                let cancellation_requested = project.task_cancelled(&ticket);
                project.active_implementations.remove(&ticket);
                project.queue.in_flight.remove(&ticket);
                if project.queue.current_ticket.as_ref() == Some(&ticket) {
                    project.queue.current_ticket = None;
                }
                {
                    if let Some(progress) = project.activity.tasks.get_mut(&ticket) {
                        progress.telemetry.finished_ms =
                            Some(chrono::Utc::now().timestamp_millis());
                        progress.activity = Some(match &result {
                            Ok(record) => record.status.label().to_owned(),
                            Err(_) => "Needs attention".into(),
                        });
                    }
                    project.save_task_activity(&ticket);
                    project.activity.dirty_tickets.remove(&ticket);
                }
                project.refresh_implementations();
                project.last_pr_refresh = None;
                let cleanup_note = result
                    .as_ref()
                    .ok()
                    .and_then(|record| record.cleanup.error.clone());
                let mut text = match result {
                    Ok(record) => {
                        project
                            .implementation_states
                            .insert(ticket.clone(), record.clone());
                        let task_event = match record.status {
                            ImplementationStatus::AwaitingApproval => Some(
                                "Implementation complete. Work is verified and saved locally; approval is required before creating a pull request.".to_owned(),
                            ),
                            ImplementationStatus::AwaitingReview => record.pr_url.as_ref().map(|url| {
                                format!("Pull request created for the verified implementation: {url}")
                            }),
                            _ => None,
                        };
                        if let Some(event) = task_event {
                            project.bind_task_conversation_identities();
                            project.task_chats.ensure_loaded(&project.chat_slug);
                            project.task_chats.remember_response(
                                &project.chat_slug,
                                &ticket,
                                vec![ChatMessage::new(
                                    ChatRole::System,
                                    event,
                                    Some(ticket.clone()),
                                )],
                            );
                        }
                        if !record.auto_merge || record.status != ImplementationStatus::Completed {
                            project.queue.running = false;
                        }
                        if crate::core::implementation::permits_evidence_only_completion(
                            &record.ticket_text,
                        ) {
                            format!(
                                "Evidence-only task verified against {} at {}. {}",
                                record.base,
                                record.merged_commit.unwrap_or_default(),
                                if project.queue.running {
                                    "Continuing the Auto queue."
                                } else {
                                    "Queue paused."
                                }
                            )
                        } else if record.auto_merge {
                            format!(
                                "Task merged into {} at {}. {}",
                                record.base,
                                record.merged_commit.unwrap_or_default(),
                                if project.queue.running {
                                    "Continuing the Auto queue."
                                } else {
                                    "Queue paused."
                                }
                            )
                        } else if matches!(
                            record.status,
                            ImplementationStatus::ReadyToPublish
                                | ImplementationStatus::AwaitingApproval
                        ) {
                            "Implementation verified and saved locally. PR approval is required before anything is shared.".into()
                        } else {
                            format!(
                                "Implementation verified. Pull request: {}",
                                record.pr_url.unwrap_or_default()
                            )
                        }
                    }
                    Err(error) => {
                        if cancellation_requested {
                            project.queue.blocked.remove(&ticket);
                            project.queue.recovery_attempts.remove(&ticket);
                            "Cancellation requested. Preserved implementation files and history; this task will not restart automatically.".into()
                        } else {
                            project.bind_task_conversation_identities();
                            project.task_chats.ensure_loaded(&project.chat_slug);
                            project.task_chats.remember_response(
                                &project.chat_slug,
                                &ticket,
                                vec![ChatMessage::new(
                                    ChatRole::System,
                                    format!("Implementation or pull request creation failed and needs attention: {}", error.message),
                                    Some(ticket.clone()),
                                )],
                            );
                            project.queue.blocked.insert(ticket.clone(), error.clone());
                            project.queue.last_error =
                                match crate::core::implementation::record_failed_attempt(
                                    &project.state.repo_root,
                                    &ticket,
                                    &error.message,
                                ) {
                                    Ok(Some(record)) => {
                                        project
                                            .implementation_states
                                            .insert(ticket.clone(), record);
                                        error.message.clone()
                                    }
                                    Ok(None) => error.message.clone(),
                                    Err(persist_error) => format!(
                                        "{}\nCould not persist the blocked retry state: {persist_error:#}",
                                        error.message
                                    ),
                                };
                            if project
                                .queue
                                .recoverable_tickets(&project.task_documents)
                                .contains(&ticket)
                            {
                                "Recoverable orchestration failure; automatically resuming preserved task work.".into()
                            } else {
                                format!(
                                    "The task needs attention after automatic recovery. Its work is preserved. Failure: {}",
                                    error.message
                                )
                            }
                        }
                    }
                };
                if let Some(error) = cleanup_note {
                    text.push_str(&format!("\nTask completed, but worktree cleanup needs attention: {error}. Cleanup will retry automatically."));
                }
                if project.queue_lock.is_some()
                    && let Err(error) = project.queue.save(&project.state.repo_root)
                {
                    project.queue.running = false;
                    project.queue.last_error.push_str(&format!("\nCannot save queue: {error}. Check disk space and permissions; this failure may not survive a restart."));
                }
                if !project.queue.running && project.active_implementations.is_empty() {
                    project.queue_lock = None;
                }
                project.activity.pending.push(text.clone());
                project.remember_chat(vec![ChatMessage::new(ChatRole::System, text, None)]);
                project.refresh_git();
            }
        }
    }
}
