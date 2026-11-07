use super::*;

pub(super) fn advance(
    project: &mut crate::app::session::Project,
    harness_cache: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
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
                project.remember_manager_activity(ChatMessage::new(ChatRole::Agent, text, None));
            }
            Err(error) => {
                let detail =
                    crate::core::context_build::clip(&crate::error::redact_secrets(&error), 1200);
                let changed = project.activity.last_manager_error.as_deref() != Some(&detail);
                project.activity.last_manager_error = Some(detail.clone());
                if changed {
                    project.remember_manager_activity(ChatMessage::new(
                        ChatRole::System,
                        format!("Manager check unavailable: {detail}"),
                        None,
                    ));
                }
            }
        }
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
    let events = std::mem::take(&mut project.activity.pending);
    let reason = events
        .iter()
        .take(3)
        .map(|event| crate::core::context_build::clip(event.lines().next().unwrap_or(event), 100))
        .collect::<Vec<_>>()
        .join(" · ");
    project.remember_manager_activity(ChatMessage::new(
        ChatRole::System,
        format!(
            "Manager check started for {} event(s): {reason}",
            events.len()
        ),
        None,
    ));
    let harness = crate::app::root::configured_harness_for_project(
        harness_cache,
        Some(crate::persistence::harness_settings::MANAGER),
        &project.state.harness_routes.routes,
    );
    let route_label = harness.label();
    project.activity.manager = Some(crate::app::manager::Manager::start(
        project, &events, harness,
    ));
    project.activity.last_update = Some(now);
    project.activity.manager_progress = crate::harness::LiveProgress {
        activity: Some(format!("Checking the project board via {route_label}")),
        selected_route: Some(route_label),
        ..Default::default()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_manager_result_is_persisted_as_activity_not_chat() {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        let home = std::env::temp_dir().join(format!("manager-result-home-{}", std::process::id()));
        environment.set("KOOLADE_HOME", &home);
        let mut app = crate::app::root::board_tests::fixture();
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let legacy = ChatMessage::new(ChatRole::Agent, "Earlier Manager patrol", None);
        project.chat.push(legacy.clone());
        let mut specification = ChatMessage::new(ChatRole::Agent, "Current answer", None);
        specification.specification_context =
            Some(crate::domain::chatlog::SpecificationChatContext {
                scope_key: "product".into(),
                intent: "ask".into(),
                published_reference: "refs/heads/main".into(),
                published_commit_sha: "a".repeat(40),
                source_documents: Vec::new(),
                effective_document_sha256: "b".repeat(64),
            });
        project.chat.push(specification.clone());
        project.activity.pending.clear();
        let chat_before = project.chat.clone();
        let planning_revision = project.state.baseline_planning_revision.clone();
        let output = crate::app::manager::ManagerOutput {
            summary: "Board checked; one blocked task needs attention.".into(),
            proposed_actions: Vec::new(),
            opportunities: Vec::new(),
            evidence_hashes: Default::default(),
            evidence_confidentialities: Default::default(),
            planning_revision,
            code_review_targets: Default::default(),
        };
        project.activity.manager =
            Some(crate::app::manager::Manager::completed_for_test(Ok(output)));
        advance(project, &mut None);
        assert_eq!(
            project.chat, chat_before,
            "Manager must not append to Specification chat"
        );
        assert_eq!(
            project.activity.manager_history.last().unwrap().text,
            "Board checked; one blocked task needs attention."
        );
        assert_eq!(
            crate::persistence::manager_activity::load(&project.chat_slug)
                .last()
                .unwrap()
                .text,
            "Board checked; one blocked task needs attention."
        );
        let (prompt_history, _) = project.recent_specification_chat("product", 12);
        assert_eq!(prompt_history.len(), 1);
        assert_eq!(prompt_history[0].1, "Current answer");
        assert!(
            project.chat.contains(&legacy),
            "legacy history remains accessible"
        );
    }

    #[test]
    fn repeated_manager_failure_does_not_duplicate_activity() {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        let home =
            std::env::temp_dir().join(format!("manager-failure-home-{}", std::process::id()));
        environment.set("KOOLADE_HOME", &home);
        let mut app = crate::app::root::board_tests::fixture();
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.activity.last_update = Some(Instant::now());
        let error = "provider is unavailable";
        project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
            error.into(),
        )));
        advance(project, &mut None);
        let history_len = project.activity.manager_history.len();
        project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
            error.into(),
        )));
        advance(project, &mut None);
        assert_eq!(project.activity.manager_history.len(), history_len);
        assert_eq!(project.activity.last_manager_error.as_deref(), Some(error));
    }
}
