use super::*;

#[test]
fn main_and_two_task_chats_accept_input_concurrently_and_cancel_independently() {
    let mut app = fixture();
    let root = std::env::temp_dir().join(format!(
        "koolade-concurrent-chats-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    if let Screen::Connected(p) = &mut app.screen {
        p.state = crate::core::state::PlannerState::load(&root).unwrap();
        p.state.bootstrap_missing().unwrap();
        p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        for id in ["CLR-001", "CLR-002"] {
            p.state.items.push(OpenItem::new(
                id.into(),
                crate::domain::Priority::High,
                crate::domain::ItemKind::Question,
                "General".into(),
                None,
                format!("Question {id}?"),
                "Planning".into(),
            ));
        }
    }
    for key in ["CLR-001", "CLR-002"] {
        app.task_harness = Some(Box::new(StoppedHarness {
            wait_for_cancel: true,
        }));
        *app.task_draft(key).unwrap() = format!("Answer for {key}");
        app.submit_task_reply(key);
        assert!(app.task_chat_active(key));
        assert!(!app.conversation_busy());
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: true,
    }));
    app.start_turn("Add a search feature");
    assert!(app.conversation_busy());
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert_eq!(p.task_turns.len(), 2);
    assert!(p.active_turn.is_some());
    assert_eq!(crate::core::planning_work::load(&root).unwrap().len(), 1);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Plan Add a search feature").is_some());
    app.cancel_task_reply("CLR-001");
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(p.task_turns["CLR-001"].cancel_requested());
    assert!(!p.task_turns["CLR-002"].cancel_requested());
    assert!(!p.active_turn.as_ref().unwrap().cancel_requested());
    app.cancel_task_reply("CLR-002");
    complete(&mut app);
    complete(&mut app);
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(
        p.active_turn.is_some(),
        "Task completion must preserve the main worker"
    );
    p.active_turn.as_ref().unwrap().request_cancel();
    drop(app);
}
