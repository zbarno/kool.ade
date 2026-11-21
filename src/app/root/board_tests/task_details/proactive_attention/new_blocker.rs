use super::*;

#[test]
fn new_blocker_opens_its_conversation_once_and_changed_blocker_surfaces_again() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = "planning/tasks/fixture/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        // The common fixture creates in-memory identities and task metadata, but
        // refresh reads the separate artifact identity marker from each file.
        for doc in &project.task_documents {
            let path = project.state.planning_store.local_root().join(&doc.path);
            let markdown = std::fs::read_to_string(&path).unwrap();
            let identity = doc.identity.as_ref().unwrap();
            let marker = format!(
                "<!-- koolade-artifact-id:v1 {} -->\n",
                serde_json::to_string(identity).unwrap()
            );
            std::fs::write(path, format!("{markdown}\n{marker}")).unwrap();
        }
        project
            .state
            .workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                identity: None,
                feature: "Synthetic fixture".into(),
                directory: "planning/tasks/fixture".into(),
                count: 3,
                created_at_ms: 1,
            });
        crate::artifacts::task_docs::save_workflow(
            &project.state.planning_store,
            &project.state.workflow,
        )
        .unwrap();
        assert_eq!(
            crate::artifacts::task_docs::load_workflow(&project.state.planning_store)
                .unwrap()
                .task_batches,
            project.state.workflow.task_batches,
            "fixture batch must survive state reload"
        );
        let reloaded_state = crate::core::state::PlannerState::load_with_store(
            &project.state.repo_root,
            &project.state.planning_store,
        )
        .unwrap();
        assert_eq!(
            reloaded_state.workflow.task_batches, project.state.workflow.task_batches,
            "fixture batch must survive a planner state reload"
        );
        let reloaded = crate::artifacts::task_docs::load_board(
            &project.state.planning_store,
            &project.state.workflow,
        );
        let actual = reloaded
            .iter()
            .map(|doc| (&doc.path, &doc.identity, &doc.metadata))
            .collect::<Vec<_>>();
        let expected = project
            .task_documents
            .iter()
            .map(|doc| (&doc.path, &doc.identity, &doc.metadata))
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "fixture tasks must survive normal background refresh"
        );
        project.git.branch = "unrefreshed-fixture".into();
    }
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
    // Exercise a real display refresh before surfacing attention. The fixture
    // must remain valid even if the background work finishes during a slow CI frame.
    app.last_git_refresh = Instant::now() - Duration::from_secs(4);
    crate::app::root::tick::poll_display_refresh_for_test(&mut app, &ctx);
    assert!(app.display_refresh.is_some(), "fixture refresh must start");
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.display_refresh.is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        crate::app::root::tick::poll_display_refresh_for_test(&mut app, &ctx);
    }
    assert!(
        app.display_refresh.is_none(),
        "fixture display refresh did not finish"
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture disconnected during display refresh")
    };
    assert_eq!(
        project.git.branch, "main",
        "display refresh must be applied"
    );
    assert!(
        project
            .task_documents
            .iter()
            .any(|doc| doc.path == key && doc.identity.is_some())
    );
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
    // A newly saved blocker and its conversation message can become visible
    // across separate tick/paint cycles. Keep polling until both effects are
    // observable rather than stopping at focus under concurrent CI load.
    let mut reopened = false;
    let mut message_persisted = false;
    for _ in 0..16 {
        app.tick(0.016, &ctx);
        frame(&mut app, &ctx, vec![]);
        reopened |= ctx
            .data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref()
            == Some(key);
        message_persisted |= match &app.screen {
            Screen::Connected(project) => project
                .task_chats
                .messages
                .get(key)
                .is_some_and(|messages| messages.len() == 2),
            _ => false,
        };
        if reopened && message_persisted {
            break;
        }
    }
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture disconnected while surfacing changed blocker");
    };
    assert_eq!(
        project
            .task_chats
            .messages
            .get(key)
            .map_or(0, |messages| messages.len()),
        2,
        "a changed blocker must persist exactly one new attention message; documents={:?}, batches={:?}, chat_keys={:?}, chat_error={:?}, blocked={:?}",
        project
            .task_documents
            .iter()
            .map(|doc| (&doc.path, &doc.identity))
            .collect::<Vec<_>>(),
        project.state.workflow.task_batches,
        project.task_chats.messages.keys().collect::<Vec<_>>(),
        project.task_chats.error,
        project.queue.blocked.keys().collect::<Vec<_>>()
    );
    assert!(
        reopened,
        "a changed blocker must focus its conversation after the board settles"
    );
}
