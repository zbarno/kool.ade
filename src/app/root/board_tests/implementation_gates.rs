use super::*;

#[path = "implementation_gates/resume_dispatch.rs"]
mod resume_dispatch;

#[test]
fn unsupported_platform_rejects_implementation_before_dispatch() {
    let mut app = fixture();
    let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities {
        planning_access: crate::harness::runtime_capabilities::PlanningAccess::SuppliedContextOnly,
        implementation: false,
    };
    app.start_implementation_with_capabilities(
        "planning/tasks/fixture/001-task.md".into(),
        false,
        capabilities,
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected");
    };
    assert!(project.active_implementations.is_empty());
    assert!(!project.queue.running);
    assert!(project.queue.in_flight.is_empty());
    assert!(project.queue.last_error.contains("Docker"));
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
    let ticket = "planning/tasks/fixture/001-task.md";
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
    assert!(!crate::core::workflow::feature_approved(
        &project.state.planning_store,
        &project.state.workflow,
        "CHG-001"
    ));
    assert!(!project.queue.last_error.is_empty());
}

#[test]
fn task_details_offer_implementation_after_generation_and_board_replaces_main_chat() {
    let mut app = fixture();
    let feature_identity = crate::domain::ArtifactIdentity::new("CHG-001", "Current feature");
    let mut batch_identity =
        crate::domain::ArtifactIdentity::new("TASK-BATCH-001", "Current feature tasks");
    batch_identity.parent_uid = Some(feature_identity.uid.clone());
    if let Screen::Connected(p) = &mut app.screen {
        p.state.workflow.brief = Some(crate::core::workflow::InterviewBrief {
            feature_name: "Current feature".into(),
            ready_for_tasks: true,
            ..Default::default()
        });
        p.state.workflow.reviewed_specification = p.state.planning_contract().map(str::to_owned);
        p.state.workflow.task_batches.clear();
        let feature = "# CHG-001: Current feature\n\n## Intent\nCurrent feature specification.\n";
        p.state.active_feature = Some(("CHG-001".into(), feature.into()));
        p.state.workflow.reviewed_specification = p.state.planning_contract().map(str::to_owned);
        let batch_dir = p
            .state
            .planning_store
            .layout()
            .task_batch("fixture")
            .unwrap();
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("contract.json"),
            serde_json::to_vec(&crate::core::contract_snapshot::BatchContract {
                feature_id: "CHG-001".into(),
                feature_specification: feature.into(),
                product_modules: Default::default(),
                repository_bases: Default::default(),
                configuration: String::new(),
            })
            .unwrap(),
        )
        .unwrap();
    }
    assert!(app.task_offer().is_some());
    assert!(!app.implementation_offer());
    if let Screen::Connected(p) = &mut app.screen {
        p.state
            .workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                identity: Some(batch_identity),
                feature: "Current feature".into(),
                directory: "planning/tasks/fixture".into(),
                count: 3,
                created_at_ms: 0,
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
fn parallel_cards_and_targeted_pause_preserves_other_workers_and_completion() {
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
    let first = "planning/tasks/fixture/001-task.md";
    let second = "planning/tasks/fixture/002-task.md";
    let mut completed_record = None;
    if let Screen::Connected(p) = &mut app.screen {
        completed_record = p.implementation_states.values().next().cloned();
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
    app.pause_task_for(first);
    if let Screen::Connected(p) = &app.screen {
        assert!(
            p.queue.running,
            "pausing one task must leave the queue running"
        );
        assert!(p.active_implementations[first].cancellation_requested());
        assert!(!p.active_implementations[second].cancellation_requested());
        assert!(!p.task_cancelled(first), "pausing remains resumable");
        assert!(p.queue.task_paused(first));
    }
    if let Screen::Connected(p) = &mut app.screen {
        let mut completed = completed_record.expect("fixture includes a saved implementation");
        completed.ticket = first.into();
        completed.ticket_text = "# First task\n".into();
        completed.status = ImplementationStatus::Completed;
        p.active_implementations.insert(
            first.into(),
            crate::core::implementation::Controller::completed_fixture(completed),
        );
    }
    crate::app::root::tick::poll_implementations_for_test(&mut app);
    if let Screen::Connected(p) = &app.screen {
        assert!(!p.queue.task_paused(first));
        assert!(!p.queue.blocked.contains_key(first));
        assert_eq!(
            p.implementation_states[first].status,
            ImplementationStatus::Completed
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
