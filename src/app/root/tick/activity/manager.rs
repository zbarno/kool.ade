use super::*;

mod recovery;

pub(super) fn advance(
    project: &mut crate::app::session::Project,
    harness_cache: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    recovery::advance_pending(project);
    if project.activity.manager.is_none() && project.activity.manager_history_error.is_some() {
        return;
    }
    let mut result = None;
    if let Some(manager) = &project.activity.manager {
        for _ in 0..64 {
            let Some(progress) = manager.progress() else {
                break;
            };
            project.activity.manager_progress.update(progress);
        }
        result = manager.result();
    }
    if let Some(result) = result {
        let (run_saved, source_events, completed_event, failed_event) = project
            .activity
            .manager
            .as_ref()
            .map(|manager| {
                (
                    manager.run_event_saved(),
                    manager.source_events().to_vec(),
                    manager.run_event(crate::domain::ManagerRunOutcomeV1::Completed, 2),
                    manager.run_event(crate::domain::ManagerRunOutcomeV1::Failed, 2),
                )
            })
            .unwrap_or_default();
        let baseline_review_id = project
            .activity
            .manager
            .as_ref()
            .and_then(crate::app::manager::Manager::baseline_review_id)
            .map(str::to_owned);
        project.activity.manager = None;
        project.activity.manager_progress = Default::default();
        match result {
            Ok(output) => {
                project.activity.last_manager_error = None;
                let dispositions = crate::app::manager::persist_dispositions(
                    project,
                    &output.proposed_actions,
                    &output.evidence_hashes,
                    &output.planning_revision,
                    &output.code_review_targets,
                );
                let mut text = crate::app::manager::persist_output(project, &output);
                match dispositions {
                    Ok(messages) if !messages.is_empty() => {
                        text.push_str("\n\n");
                        text.push_str(&messages.join("\n"));
                    }
                    Err(error) => text.push_str(&format!(
                        "\n\nManager decisions could not be saved to the Board: {error}"
                    )),
                    _ => {}
                }
                let mut message = ChatMessage::new(ChatRole::Agent, text, None);
                message.verified_repository_evidence = output.verified_repository_evidence;
                message.baseline_review_id = output.baseline_review_id;
                if run_saved {
                    if let Some(event) = completed_event {
                        if let Err(error) =
                            project.remember_manager_run_message(message.clone(), event)
                        {
                            project.activity.manager_history_error = Some(format!(
                                "Manager output was applied but its durable completion event could not be saved: {error}"
                            ));
                            project.activity.pending.extend(source_events.clone());
                            project.activity.manager_history.push(message);
                            if project.activity.manager_history.len() > 300 {
                                project.activity.manager_history.remove(0);
                            }
                        }
                    } else {
                        project.remember_manager_activity(message);
                    }
                } else {
                    project.remember_manager_activity(message);
                }
            }
            Err(error) => {
                let detail =
                    crate::core::context_build::clip(&crate::error::redact_secrets(&error), 1200);
                if run_saved && let Some(event) = failed_event {
                    let checkpoint_hash = event.checkpoint_sha256.clone();
                    let attempts = crate::domain::chatlog::checkpoint_attempts(
                        &project.activity.manager_history,
                        &checkpoint_hash,
                    );
                    let saved = project
                        .persist_manager_run_event(event.clone(), "Manager check failed.")
                        .is_ok();
                    if saved && attempts < 2 {
                        let mut retry = event;
                        retry.sequence = 3;
                        retry.outcome = crate::domain::ManagerRunOutcomeV1::Retried;
                        retry.checkpoint = None;
                        let retry_saved = project
                            .persist_manager_run_event(
                                retry,
                                "Manager will retry this saved event batch once.",
                            )
                            .is_ok();
                        if !retry_saved {
                            project.activity.manager_history_error = Some(
                                "Manager retry marker could not be saved; its source events were returned to the queue.".into(),
                            );
                        }
                        project.activity.pending.extend(source_events.clone());
                    }
                } else {
                    project.activity.pending.extend(source_events.clone());
                }
                let changed = project.activity.last_manager_error.as_deref() != Some(&detail);
                project.activity.last_manager_error = Some(detail.clone());
                if changed || baseline_review_id.is_some() {
                    let mut message = ChatMessage::new(
                        ChatRole::System,
                        format!("Manager check unavailable: {detail}"),
                        None,
                    );
                    message.baseline_review_id = baseline_review_id;
                    project.remember_manager_activity(message);
                }
            }
        }
    }

    if project.activity.manager_history_error.is_some() {
        return;
    }
    let now = Instant::now();
    if crate::app::manager::patrol_note_due(
        project.active_implementations.len(),
        project.activity.pending.len(),
        project.activity.last_update,
        project.activity.last_patrol_note,
        now,
    ) {
        project.activity.pending.push("The worker is still running. No completion is confirmed; review the current task states and help the user with the next eligible planning decision without inventing progress.".into());
        project.activity.last_patrol_note = Some(now);
    }
    if !crate::app::manager::patrol_manager_due(
        project.active_turn.is_some(),
        project.activity.manager.is_some(),
        project.activity.pending.len(),
        project.activity.last_update,
        now,
    ) {
        return;
    }

    project.bind_task_conversation_identities();
    project.task_chats.refresh_now(&project.chat_slug);
    project
        .activity
        .pending
        .extend(project.task_chats.take_updates());
    let events = project.activity.pending.clone();
    if events.is_empty() {
        return;
    }
    let reason = events
        .iter()
        .take(3)
        .map(|event| crate::core::context_build::clip(event.lines().next().unwrap_or(event), 100))
        .collect::<Vec<_>>()
        .join(" · ");
    let harness = crate::app::root::configured_harness_for_project(
        harness_cache,
        Some(crate::persistence::harness_settings::MANAGER),
        &project.state.harness_routes.routes,
    );
    let route_label = harness.label();
    let baseline_review = project.activity.pending_baseline_review.clone();
    let binding = harness.capability_refresh_binding();
    let checkpoint = match crate::domain::ManagerEventCheckpointV1::from_messages(
        &events,
        binding,
        baseline_review
            .as_ref()
            .map(|review| review.review_id.clone()),
        baseline_review
            .as_ref()
            .map(|review| review.repository_id.clone()),
    ) {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            project.activity.last_manager_error = Some(format!(
                "Manager event batch could not be checkpointed: {error}"
            ));
            project.activity.last_update = Some(now);
            return;
        }
    };
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let route_blocker = harness.route_blocker();
    let outcome = if route_blocker.is_some() {
        crate::domain::ManagerRunOutcomeV1::Queued
    } else {
        crate::domain::ManagerRunOutcomeV1::Started
    };
    let start_event = crate::domain::ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence: 1,
        outcome,
        checkpoint: Some(checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: route_blocker
            .as_ref()
            .map(|_| crate::persistence::harness_settings::current_unix_ms().saturating_add(5_000)),
    };
    let start_text = if let Some(blocker) = &route_blocker {
        format!("Manager is waiting for its selected route: {blocker}")
    } else {
        format!(
            "Manager check started for {} event(s): {reason}",
            events.len()
        )
    };
    if let Err(error) = project.persist_manager_run_event(start_event, start_text) {
        project.activity.last_manager_error = Some(format!(
            "Manager events remain queued because their durable checkpoint could not be saved: {error}"
        ));
        project.activity.last_update = Some(now);
        return;
    }
    project.activity.pending.drain(..events.len());
    project.activity.pending_baseline_review = None;
    project.activity.last_update = Some(now);
    project.activity.manager_progress = crate::harness::LiveProgress {
        activity: Some(if route_blocker.is_some() {
            "Recovering selected Manager capability proof".into()
        } else {
            format!("Checking the project board via {route_label}")
        }),
        selected_route: Some(route_label),
        ..Default::default()
    };
    if route_blocker.is_some() {
        return;
    }
    project.activity.manager = Some(crate::app::manager::Manager::start_supervised(
        project,
        &events,
        baseline_review,
        harness,
        run_id,
        checkpoint,
    ));
}

#[cfg(test)]
mod tests;
