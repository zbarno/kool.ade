use super::*;

#[test]
fn question_task_persists_starts_async_answers_and_finishes_without_a_spec() {
    let root = std::env::temp_dir().join(format!(
        "koolade-new-question-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "main"],
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
        project.refresh_git();
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
        "Source Branch",
        "Destination Branch",
        "Create Task",
        "Plan a capability or improve how the project works.",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    click_text(&mut app, &ctx, "Question task");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Get an answer grounded in this project.").is_some());
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
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(dialog),
            "dialog {dialog:?} exceeded viewport {size:?}"
        );
    }
    let dialog = ctx
        .memory(|memory| memory.area_rect(initial_window_id))
        .unwrap();
    super::super::board_tests::frame_at(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(dialog.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1200.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            },
        ],
        egui::vec2(1800.0, 900.0),
    );
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
