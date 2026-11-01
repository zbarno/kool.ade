use super::*;

fn legacy_clarification_prompt_id(item: &crate::domain::OpenItem) -> String {
    // Prior versions keyed proactive questions by display ID and question
    // copy. This fixture reproduces that format to cover upgrade behavior.
    let decision = serde_json::to_string(&item.decision_brief).unwrap_or_default();
    let detail = format!("{}\n{}\n{decision}", item.question, item.reason);
    let mut material = detail.into_bytes();
    material.extend_from_slice(item.conversation_key().as_bytes());
    material.push(2);
    let mut hash = 0xcbf29ce484222325u64;
    for byte in &material {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("needs-attention:{hash:016x}-{}", material.len())
}

#[path = "proactive_attention/recurrence.rs"]
mod recurrence;
#[path = "proactive_attention/reply_pending.rs"]
mod reply_pending;

#[test]
fn new_blocker_opens_its_conversation_once_and_changed_blocker_surfaces_again() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = "planning/tasks/fixture/001-task.md";
    let detail = "## Waiting for user action\n\nThe provider quota stopped this task.\n\n### Next action(s)\n\n- Account owner: choose whether to wait or request more capacity.";
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.blocked.insert(
            key.into(),
            crate::core::implementation::Failure::other(detail),
        );
    }
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "The provider quota stopped this task.".into(),
            recommendation: None,
            options: Vec::new(),
            steps: vec![crate::core::attention::HumanStep {
                owner: "Account owner".into(),
                action: "Choose whether to wait or request more capacity.".into(),
            }],
            after: "Resume once the quota is available.".into(),
        },
    );
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(key)
    );
    for section in [
        "Account owner: Choose whether to wait or request more capacity.",
        "I need your help before I continue.",
    ] {
        assert!(
            text_contains(&output, section),
            "missing {section}: {}",
            canvas_text(&output)
        );
    }

    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .is_none()
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages[key].len(), 1);
    }

    let changed = format!("{detail}\n\nNew recovery attempt is available.");
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.blocked.insert(
            key.into(),
            crate::core::implementation::Failure::other(changed),
        );
    }
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "A new recovery attempt is available.".into(),
            recommendation: None,
            options: Vec::new(),
            steps: vec![crate::core::attention::HumanStep {
                owner: "You".into(),
                action: "Retry the task.".into(),
            }],
            after: "Review the task result.".into(),
        },
    );
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(key)
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages[key].len(), 2);
    }
}

#[test]
fn approval_gate_opens_a_task_chat_with_the_matching_user_action() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = "planning/tasks/fixture/002-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        let record = project.implementation_states.get_mut(key).unwrap();
        record.status = crate::core::implementation::ImplementationStatus::AwaitingApproval;
        record.pr_url = None;
        record.pr_state = None;
    }
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for text in [
        "Implementation is complete and verified",
        "Approve and create pull request",
        "Request changes",
    ] {
        assert!(text_contains(&output, text), "missing {text}");
    }
}

#[test]
fn multiple_attention_events_open_one_conversation_after_another() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let first = "planning/tasks/fixture/001-task.md";
    let second = "planning/tasks/fixture/002-task.md";
    let brief = || crate::core::attention::Brief {
        problem: "A provider limit paused this task.".into(),
        recommendation: None,
        options: Vec::new(),
        steps: vec![crate::core::attention::HumanStep {
            owner: "You".into(),
            action: "Choose when to retry.".into(),
        }],
        after: "Resume when ready.".into(),
    };
    if let Screen::Connected(project) = &mut app.screen {
        for key in [first, second] {
            project.queue.blocked.insert(
                key.into(),
                crate::core::implementation::Failure::other("Provider limit reached."),
            );
        }
    }
    app.attention_fixture.insert(first.into(), brief());
    app.attention_fixture.insert(second.into(), brief());
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(first)
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(second)
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages[first].len(), 1);
        assert_eq!(project.task_chats.messages[second].len(), 1);
    }
}

#[test]
fn agent_owned_follow_up_does_not_prompt_the_user_or_open_attention() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let mut item = crate::domain::OpenItem::new(
        "CLR-006".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("Zach".into()),
        "Update the existing spec with the previously settled default.".into(),
        "A related clarification was resolved; this is agent-owned follow-up.".into(),
    );
    item.authority = crate::domain::Authority::Agent;
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
    }

    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);

    assert_ne!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-006"),
        "an Agent item must not open as a human clarification"
    );
    if let Screen::Connected(project) = &app.screen {
        assert!(
            !project.task_chats.messages.contains_key("CLR-006"),
            "an Agent item must not receive a synthetic 'answer this question' chat message"
        );
    }
}

#[test]
fn newly_actionable_clarification_focuses_its_persistent_conversation() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-030".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Should this release include the migration?".into(),
        "The answer changes the release steps.".into(),
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
    }
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-030")
    );
    for text in [
        "Should this release include the migration?",
        "Ask a question",
    ] {
        assert!(text_contains(&output, text), "missing {text}");
    }
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages["CLR-030"].len(), 1);
    }
}
