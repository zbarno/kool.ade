use super::*;

#[test]
fn a_stranded_last_reply_drains_before_the_instance_is_replaced() {
    // Incident regression: one save hiccups and queues the final agent reply
    // as pending; unpainted-panel retry paths may never run, and a later
    // TaskChats replacement (relaunch-style) must not drop the reply. The
    // per-frame paint drain must deliver it once the store unlocks.
    let root = std::env::temp_dir().join(format!(
        "koolade-stranded-drain-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    let question = "Which database engine?";
    if let Screen::Connected(p) = &mut app.screen {
        p.state = crate::core::state::PlannerState::load(&repo).unwrap();
        p.state.bootstrap_missing().unwrap();
        p.state.items = vec![OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            question.into(),
            "Storage choice".into(),
        )];
        p.task_documents.clear();
        p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
    }
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::new(Mutex::new(Vec::new())),
        reply: serde_json::json!({
            "schema_version":1, "assistant_message":"Postgres chosen and noted.",
            "open_items_updated":[{"id":"CLR-001","evidence":"Operator picked Postgres."}]
        })
        .to_string(),
    }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Your answer…");
    frame(&mut app, &ctx, vec![egui::Event::Text("Postgres".into())]);
    assert_eq!(app.task_draft("CLR-001").unwrap(), "Postgres");
    click_text(&mut app, &ctx, question);
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
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Send answer");
    complete(&mut app);
    assert!(
        app.task_messages("CLR-001")
            .iter()
            .any(|m| m.text == "Postgres chosen and noted.")
    );
    let persisted_len =
        persisted_len_helper(&app, crate::persistence::task_chats::TaskChats::default());
    // Straddle: hold the store lock ourselves, then let one agent reply fail
    // into the pending queue — memory shows it, disk does not.
    let slug = match &app.screen {
        Screen::Connected(p) => p.chat_slug.clone(),
        _ => panic!("expected connected screen"),
    };
    let lock = {
        let dir = crate::persistence::project_dir(&slug);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("task-conversations.lock"))
            .unwrap();
        file.lock().unwrap();
        file
    };
    let memory_len;
    {
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        p.task_chats.remember_response(
            &p.chat_slug,
            "CLR-001",
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Stranded late reply",
                None,
            )],
        );
        assert!(
            p.task_chats
                .error
                .as_deref()
                .is_some_and(|e| e.contains("Another window")),
            "expected the reply to fail into the pending queue, got {:?}",
            p.task_chats.error
        );
        memory_len = p.task_chats.messages["CLR-001"].len();
    }
    assert_eq!(memory_len, persisted_len + 1);
    // Frames while the store is locked keep the reply pending, invisible to a
    // fresh reader (a relaunched window).
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        persisted_len_helper(&app, crate::persistence::task_chats::TaskChats::default()),
        persisted_len
    );
    drop(lock);
    // The next painted frame drains the queue, so even a hard instance swap
    // now reads the complete history.
    frame(&mut app, &ctx, vec![]);
    {
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        p.task_chats = Default::default();
        p.task_chats.ensure_loaded(&p.chat_slug);
        assert_eq!(p.task_chats.messages["CLR-001"].len(), memory_len);
        assert!(
            p.task_chats.messages["CLR-001"]
                .iter()
                .any(|m| m.text == "Stranded late reply")
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
