use super::*;

#[test]
fn unsupported_platform_rejects_implementation_before_dispatch() {
    let mut app = fixture();
    let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities {
        planning_access: crate::harness::runtime_capabilities::PlanningAccess::SuppliedContextOnly,
        implementation: false,
    };
    app.start_implementation_with_capabilities(
        ".koolade-packet/planning/tasks/fixture/001-task.md".into(),
        false,
        capabilities,
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(project.active_implementations.is_empty());
    assert!(!project.queue.running);
    assert!(project.queue.in_flight.is_empty());
    assert!(project.queue.last_error.contains("Bubblewrap"));
    assert!(
        project
            .queue
            .last_error
            .contains("planned artifacts are preserved")
    );
}

#[test]
fn auto_queue_cannot_start_task_from_unapproved_feature() {
    let mut app = fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        assert!(project.queue.auto_build);
        project.task_documents[0]
            .text
            .push_str("\nFeature ID: CHG-001\n");
    }
    app.implement_task(ticket.into());
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(project.active_implementations.is_empty());
    assert!(!project.queue.running);
    assert!(project.queue.last_error.contains("needs explicit approval"));
}

#[test]
fn resume_dispatch_accepts_feature_named_workspace_story() {
    let _shield = crate::core::gitops::test_support::shield("resume-feature-ticket");
    let root = std::env::temp_dir().join(format!(
        "koolade-resume-dispatch-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let ticket = ".koolade-packet/planning/tasks/demo/CHG-003-TASK-verify.md";
    std::fs::create_dir_all(root.join(ticket).parent().unwrap()).unwrap();
    std::fs::write(root.join(ticket), "# Verify workspace\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        let mut record = p.implementation_states.values().next().unwrap().clone();
        record.ticket = ticket.into();
        record.ticket_text = "# Verify workspace\n".into();
        record.status = ImplementationStatus::Blocked;
        record.pr_url = None;
        p.state = crate::core::state::PlannerState::load(&root).unwrap();
        p.task_documents = vec![crate::artifacts::task_docs::TaskDocument {
            path: ticket.into(),
            title: "Verify workspace".into(),
            text: record.ticket_text.clone(),
            identity: None,
            metadata: None,
            metadata_error: None,
        }];
        p.implementation_states = [(ticket.into(), record)].into();
        p.queue.blocked.insert(
            ticket.into(),
            crate::core::implementation::Failure::other("Previous failure"),
        );
        p.queue.recovery_attempts.insert(ticket.into(), 1);
    }
    app.implement_task(ticket.into());
    let Screen::Connected(p) = &mut app.screen else {
        panic!("disconnected")
    };
    assert!(
        p.active_implementations.contains_key(ticket),
        "{}",
        p.queue.last_error
    );
    assert!(!p.queue.blocked.contains_key(ticket));
    assert!(!p.queue.recovery_attempts.contains_key(ticket));
    let controller = p.active_implementations.remove(ticket).unwrap();
    controller.request_cancel();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(
            controller.poll(),
            Some(crate::core::implementation::Event::Done(_))
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture worker failed to stop");
        std::thread::sleep(Duration::from_millis(10));
    }
    // No remote is configured: this dispatch test cannot launch Pi or publish.
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_details_offer_implementation_after_generation_and_board_replaces_main_chat() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.state.workflow.brief = Some(crate::core::workflow::InterviewBrief {
            feature_name: "Current feature".into(),
            ready_for_tasks: true,
            ..Default::default()
        });
        p.state.workflow.reviewed_specification = p.state.planning_contract().map(str::to_owned);
        p.state.workflow.task_batches.clear();
        p.state.active_feature = Some(("CHG-001".into(), "Current feature specification".into()));
        p.state.workflow.reviewed_specification = p.state.planning_contract().map(str::to_owned);
    }
    assert!(app.task_offer().is_some());
    assert!(!app.implementation_offer());
    if let Screen::Connected(p) = &mut app.screen {
        p.state
            .workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                identity: None,
                feature: "Current feature".into(),
                directory: ".koolade-packet/planning/tasks/fixture".into(),
                count: 3,
            });
    }
    assert!(
        app.task_offer().is_none(),
        "stale readiness must not offer duplicate generation"
    );
    assert!(app.implementation_offer());
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Main Chat").is_none());
    let output = click_text(&mut app, &ctx, "First task");
    assert!(text_position(&output, "Implement & continue queue").is_some());
    assert!(text_position(&output, "Implement tasks").is_none());
    assert!(text_position(&output, "Generate task stories").is_none());
    if let Screen::Connected(p) = &mut app.screen {
        let mut done = p.implementation_states.values().next().unwrap().clone();
        done.status = ImplementationStatus::Completed;
        for doc in &p.task_documents {
            p.implementation_states
                .insert(doc.path.clone(), done.clone());
        }
    }
    assert!(!app.implementation_offer());
    assert!(app.task_offer().is_none());
}

#[test]
fn typed_start_action_without_tasks_explains_the_live_blocker() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.task_documents.clear();
    }
    requested_action::dispatch(
        &mut app,
        crate::harness::RequestedAction {
            action: crate::harness::ApplicationAction::StartImplementation,
            target_uid: None,
        },
    );
    let Screen::Connected(p) = &app.screen else {
        panic!("disconnected")
    };
    assert!(p.active_turn.is_none());
    assert!(p.active_implementations.is_empty());
    assert!(!p.queue.running);
    assert!(
        app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("There is no task batch")
    );
}

#[test]
fn parallel_cards_and_targeted_cancel_preserve_other_workers() {
    let _shield = crate::core::gitops::test_support::shield("parallel-cancel-one-task");
    let mut app = fixture();
    let root = std::env::temp_dir().join(format!(
        "koolade-parallel-cancel-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let first = ".koolade-packet/planning/tasks/fixture/001-task.md";
    let second = ".koolade-packet/planning/tasks/fixture/002-task.md";
    if let Screen::Connected(p) = &mut app.screen {
        p.state.repo_root = root.clone();
        p.implementation_states.remove(second);
        p.queue.max_parallel = 2;
        p.queue.running = true;
        for ticket in [first, second] {
            p.active_implementations.insert(
                ticket.into(),
                crate::core::implementation::Controller::idle_fixture(),
            );
        }
    }
    assert!(!app.implementation_capacity());
    assert!(app.implementation_active(first) && app.implementation_active(second));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "In progress · 2").is_some());
    app.cancel_task_for(first);
    if let Screen::Connected(p) = &app.screen {
        assert!(
            p.queue.running,
            "cancelling one task must leave the queue running"
        );
        assert!(p.active_implementations[first].cancellation_requested());
        assert!(!p.active_implementations[second].cancellation_requested());
        assert!(
            p.cancelled_work
                .contains(&crate::persistence::cancelled_work::task_id(
                    &p.task_documents[0]
                ))
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
