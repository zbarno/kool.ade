use super::*;
use std::sync::{Arc, Mutex};

#[test]
fn feature_bug_and_new_project_tasks_persist_their_kind_and_start_from_the_board() {
    let root = std::env::temp_dir().join(format!(
        "koolade-new-kinds-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Kool.ad/e Task Test"],
        vec!["config", "user.email", "koolade-task@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
    }
    let ctx = egui::Context::default();
    for (label, kind, description) in [
        (
            "Feature",
            crate::core::planning_work::WorkKind::Feature,
            "Export project tasks",
        ),
        (
            "Bug",
            crate::core::planning_work::WorkKind::Bug,
            "Fix wrapped blocker details",
        ),
        (
            "New Project",
            crate::core::planning_work::WorkKind::NewProject,
            "Plan a local notes app",
        ),
        (
            "Refresh Documentation",
            crate::core::planning_work::WorkKind::DocumentationRefresh,
            "Document the repository and triage findings",
        ),
    ] {
        app.task_harness = Some(Box::new(StoppedHarness {
            wait_for_cancel: true,
        }));
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "+ New Task");
        click_text(&mut app, &ctx, label);
        click_text(&mut app, &ctx, "Describe what you want to do…");
        frame(&mut app, &ctx, vec![egui::Event::Text(description.into())]);
        click_text(&mut app, &ctx, "Create Task");
        let uid = {
            let Screen::Connected(project) = &app.screen else {
                panic!()
            };
            assert!(
                project.active_turn.is_some(),
                "{label} starts planning immediately"
            );
            let created = project.planning_work.last().unwrap();
            assert_eq!(created.kind, kind);
            assert_eq!(created.request, description);
            let persisted = crate::core::planning_work::load(&root).unwrap();
            assert_eq!(persisted.last().unwrap().uid, created.uid);
            created.uid.clone()
        };
        if let Screen::Connected(project) = &app.screen {
            project.active_turn.as_ref().unwrap().request_cancel();
        }
        complete(&mut app);
        assert!(
            crate::core::planning_work::load(&root)
                .unwrap()
                .iter()
                .any(|work| work.uid == uid && work.kind == kind)
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn question_task_persists_starts_async_answers_and_finishes_without_a_spec() {
    let root = std::env::temp_dir().join(format!(
        "koolade-new-question-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Kool.ad/e Question Test"],
        vec!["config", "user.email", "koolade-question@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
    }
    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::clone(&prompts),
        reply: serde_json::json!({
            "schema_version": 2,
            "assistant_message": "SQLite is used because the application needs transactional local storage.",
            "document_updates": [],
            "open_items_added": [],
            "open_items_updated": [],
            "open_items_resolved": [],
            "next_question_id": null,
            "requested_action": null
        })
        .to_string(),
    }));

    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "+ New Task").is_some());
    click_text(&mut app, &ctx, "+ New Task");
    let output = frame(&mut app, &ctx, vec![]);
    for label in [
        "Feature",
        "Bug",
        "New Project",
        "Question task",
        "Create Task",
        "Plan a new capability or improve how your project works.",
        "Investigate something that is broken and plan a fix.",
        "Define a project's purpose, scope, and architecture. Use this to start documenting an existing codebase too.",
        "Get an answer grounded in your project. This task answers questions without creating or updating specifications.",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    let initial_window_id = egui::Id::new("koolade_modal").with("New Task");
    let dialog = ctx
        .memory(|memory| memory.area_rect(initial_window_id))
        .unwrap();
    assert!(dialog.center().distance(egui::pos2(900.0, 450.0)) < 2.0);
    for size in [egui::vec2(360.0, 480.0), egui::vec2(1800.0, 900.0)] {
        for _ in 0..3 {
            super::super::board_tests::frame_at(&mut app, &ctx, vec![], size);
        }
        let window_id = egui::Id::new("koolade_modal").with("New Task");
        let dialog = ctx.memory(|memory| memory.area_rect(window_id)).unwrap();
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(dialog));
    }
    click_text(&mut app, &ctx, "Question task");
    click_text(&mut app, &ctx, "Describe what you want to do…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text(
            "Why does the application use SQLite?".into(),
        )],
    );
    click_text(&mut app, &ctx, "Create Task");
    let key = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected");
        };
        assert!(project.active_turn.is_some(), "planning starts immediately");
        let item = project.planning_work.last().unwrap();
        assert_eq!(item.kind, crate::core::planning_work::WorkKind::Question);
        assert_eq!(item.request, "Why does the application use SQLite?");
        let saved = crate::core::planning_work::load(&root).unwrap();
        assert_eq!(saved.last().unwrap().uid, item.uid);
        assert_eq!(saved.last().unwrap().kind, item.kind);
        item.key.clone()
    };

    complete(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    let item = project
        .planning_work
        .iter()
        .find(|item| item.key == key)
        .unwrap();
    assert_eq!(item.status, crate::core::planning_work::WorkStatus::Done);
    assert!(item.feature_id.is_none());
    assert!(item.detail.contains("SQLite is used"));
    assert!(project.state.active_features.is_empty());
    let restarted = crate::core::planning_work::load(&root).unwrap();
    let restored = restarted
        .iter()
        .find(|restored| restored.key == key)
        .unwrap();
    assert_eq!(restored.uid, item.uid);
    assert_eq!(
        restored.kind,
        crate::core::planning_work::WorkKind::Question
    );
    assert_eq!(
        restored.status,
        crate::core::planning_work::WorkStatus::Done
    );
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("Task kind and guidance: Question"));
    assert!(prompts[0].contains("Do not create or update feature specifications"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn question_answer_can_offer_a_linked_feature_task_that_starts_only_on_selection() {
    let root = std::env::temp_dir().join(format!(
        "koolade-question-followup-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Kool.ad/e Follow-up Test"],
        vec!["config", "user.email", "koolade-followup@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
    }
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::new(Mutex::new(Vec::new())),
        reply: serde_json::json!({
            "schema_version": 2,
            "assistant_message": "Hosted Anthropic is not supported by the current provider relay. Create a Feature task if you want Kool.ad/e to plan that capability.",
            "document_updates": [],
            "open_items_added": [],
            "open_items_updated": [],
            "open_items_resolved": [],
            "next_question_id": null,
            "requested_action": null
        })
        .to_string(),
    }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "+ New Task");
    click_text(&mut app, &ctx, "Question task");
    click_text(&mut app, &ctx, "Describe what you want to do…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text(
            "Why can't Kool.ad/e use hosted Anthropic through Pi?".into(),
        )],
    );
    click_text(&mut app, &ctx, "Create Task");
    complete(&mut app);

    let parent_uid = {
        let Screen::Connected(project) = &app.screen else {
            panic!()
        };
        let parent = project.planning_work.last().unwrap();
        assert_eq!(parent.kind, crate::core::planning_work::WorkKind::Question);
        assert_eq!(parent.status, crate::core::planning_work::WorkStatus::Done);
        let offer = parent.follow_up_task.as_ref().unwrap();
        assert_eq!(offer.title, "Add hosted Anthropic support through Pi");
        parent.uid.clone()
    };
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Create related Feature task").is_some());
    click_text(&mut app, &ctx, "Create related Feature task");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        project.active_turn.is_some(),
        "the selected offer starts planning"
    );
    let child = project.planning_work.last().unwrap();
    assert_eq!(child.kind, crate::core::planning_work::WorkKind::Feature);
    assert_eq!(child.parent_uid.as_deref(), Some(parent_uid.as_str()));
    assert_eq!(
        child.request,
        "Plan support for hosted Anthropic through Pi while preserving Kool.ad/e's sandbox security boundaries."
    );
    let parent = project
        .planning_work
        .iter()
        .find(|work| work.uid == parent_uid)
        .unwrap();
    assert!(
        parent.follow_up_task.is_none(),
        "the offer cannot be selected twice"
    );
    project.active_turn.as_ref().unwrap().request_cancel();
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}
