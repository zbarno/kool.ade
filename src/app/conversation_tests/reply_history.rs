use super::*;

#[test]
fn inline_and_modal_replies_share_history_and_keep_other_chats_out_of_prompts() {
    let root = std::env::temp_dir().join(format!("koolade_conversation_ui_{}", std::process::id()));
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
    let question = "Which authentication provider?";
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
            "Controls access".into(),
        )];
        p.task_documents.clear();
        p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        p.chat = vec![ChatMessage::new(
            ChatRole::User,
            "MAIN CHAT PRIVATE SENTINEL",
            None,
        )];
        p.task_chats
            .append(
                &p.chat_slug,
                "CLR-002",
                vec![ChatMessage::new(
                    ChatRole::User,
                    "OTHER TASK PRIVATE SENTINEL",
                    None,
                )],
            )
            .unwrap();
    }
    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness { prompts: prompts.clone(), reply: serde_json::json!({
        "schema_version":1, "assistant_message":"Corporate SSO recorded. Any additional requirement?",
        "open_items_updated":[{"id":"CLR-001", "evidence":"Use corporate SSO"}]
    }).to_string() }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Your answer…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Use corporate SSO".into())],
    );
    assert_eq!(app.task_draft("CLR-001").unwrap(), "Use corporate SSO");
    click_text(&mut app, &ctx, question);
    assert!(text_position(&frame(&mut app, &ctx, vec![]), "Use corporate SSO").is_some());
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
    assert_eq!(app.task_messages("CLR-001").len(), 1);
    complete(&mut app);
    assert_eq!(app.task_messages("CLR-001").len(), 3);
    assert_eq!(app.chat_messages().len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Needs attention · 1").is_some());
    click_text(&mut app, &ctx, question);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Your answer needed").is_some());
    assert!(text_position(&output, "Use corporate SSO").is_some());
    click_last(&mut app, &ctx, "Your answer…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Require MFA as well".into())],
    );
    assert_eq!(app.task_draft("CLR-001").unwrap(), "Require MFA as well");
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
    assert!(text_position(&frame(&mut app, &ctx, vec![]), "Require MFA as well").is_some());
    click_text(&mut app, &ctx, question);
    app.task_harness = Some(Box::new(ReplyHarness { prompts: prompts.clone(), reply: serde_json::json!({
        "schema_version":1, "assistant_message":"Corporate SSO with MFA is confirmed.",
        "document_updates":[{"document_id":"product:current-capabilities","content":"## 5. Functional Requirements\n\nUse corporate SSO with MFA.\n"}],
        "open_items_resolved":["CLR-001"]
    }).to_string() }));
    click_last(&mut app, &ctx, "Send answer");
    assert_eq!(app.task_messages("CLR-001").len(), 4);
    complete(&mut app);
    assert_eq!(app.task_messages("CLR-001").len(), 6);
    assert_eq!(app.chat_messages().len(), 1);
    let captured = prompts.lock().unwrap();
    assert_eq!(captured.len(), 2);
    for prompt in captured.iter() {
        assert!(!prompt.contains("MAIN CHAT PRIVATE SENTINEL"));
        assert!(!prompt.contains("OTHER TASK PRIVATE SENTINEL"));
    }
    assert!(captured[1].contains("Corporate SSO recorded. Any additional requirement?"));
    drop(captured);
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let persisted = crate::core::state::PlannerState::load(&repo).unwrap();
    assert!(persisted.spec_text.unwrap().contains("SSO with MFA"));
    assert_eq!(persisted.resolved_items[0].conversation_key(), "CLR-001");
    p.task_chats = Default::default();
    p.task_chats.ensure_loaded(&p.chat_slug);
    assert_eq!(p.task_chats.messages["CLR-001"].len(), 6);
    assert_eq!(p.task_chats.messages["CLR-002"].len(), 1);
    let manager_prompt = crate::app::manager::Manager::prompt_body(p.as_ref(), &p.activity.pending);
    for fact in [
        "Use corporate SSO",
        "Require MFA as well",
        "This question is resolved.",
        "CLR-001",
    ] {
        assert!(manager_prompt.contains(fact), "manager missed {fact}");
    }
    assert!(
        p.activity
            .pending
            .iter()
            .any(|event| event.contains("User replied in task CLR-001"))
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Done · 1").is_some());
    assert!(text_position(&output, "Corporate SSO with MFA is confirmed.").is_some());
    let main_prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: main_prompts.clone(),
        reply: serde_json::json!({"schema_version":1, "assistant_message":"The task conversation confirmed corporate SSO with MFA."}).to_string(),
    }));
    app.start_turn_with_purpose(
        "What did we decide in CLR-001?",
        crate::core::workflow::TurnPurpose::Interview,
    );
    complete(&mut app);
    let main_prompts = main_prompts.lock().unwrap();
    assert_eq!(main_prompts.len(), 1);
    for fact in [
        "Use corporate SSO",
        "Require MFA as well",
        "This question is resolved.",
        "OTHER TASK PRIVATE SENTINEL",
    ] {
        assert!(main_prompts[0].contains(fact), "main planner missed {fact}");
    }
    std::fs::remove_dir_all(root).unwrap();
}
