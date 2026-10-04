use super::*;

#[test]
fn new_task_tabs_show_context_and_help_without_starting_a_model_turn() {
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Open conversation");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Task context").is_some());
    assert!(
        app.task_messages(key)[0]
            .text
            .contains("How can I help you with First task?")
    );
    assert!(!app.task_reply_busy());
    assert!(app.chat_messages().is_empty());
    let context = app.task_chat_context(key).unwrap();
    assert!(context.contains("Unique story detail 0"));
    assert!(!context.contains("Unique story detail 1"));
    let expected = app.task_messages(key).to_vec();
    app.prepare_task_chat(key);
    assert_eq!(app.task_messages(key), expected);
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let slug = p.chat_slug.clone();
    p.task_chats = Default::default();
    app.prepare_task_chat(key);
    assert_eq!(app.task_messages(key), expected);
    std::fs::remove_dir_all(slug).unwrap();
}

#[test]
fn question_opening_uses_current_context_and_resolved_items_offer_help() {
    let mut app = fixture();
    let key = "CLR-001";
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let mut item = OpenItem::new(
        key.into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which authentication provider?".into(),
        "Controls employee access".into(),
    );
    item.evidence = "Corporate directory is available".into();
    p.state.items.push(item.clone());
    app.prepare_task_chat(key);
    let greeting = &app.task_messages(key)[0].text;
    assert!(greeting.contains("Controls employee access"));
    assert!(greeting.contains("- Which authentication provider?"));
    assert!(
        app.task_chat_context(key)
            .unwrap()
            .contains("Corporate directory is available")
    );
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    p.task_documents[0]
        .text
        .push_str("\nPending decision: CLR-001");
    let task = p.task_documents[0].path.clone();
    app.prepare_task_chat(&task);
    assert!(
        app.task_messages(&task)[0]
            .text
            .contains("- CLR-001: Which authentication provider?")
    );
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    item.id = "CLR-002".into();
    item.status = crate::domain::ItemStatus::Resolved;
    p.state.resolved_items.push(item);
    app.prepare_task_chat("CLR-002");
    let greeting = &app.task_messages("CLR-002")[0].text;
    assert!(greeting.contains("- How can I help you with this item?"));
    assert!(!greeting.contains("- Which authentication provider?"));
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    std::fs::remove_dir_all(&p.chat_slug).unwrap();
}

#[test]
fn tabs_select_persisted_item_histories_without_loading_main_chat() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let first = ".koolade-packet/planning/tasks/fixture/001-task.md";
    let second = ".koolade-packet/planning/tasks/fixture/002-task.md";
    let dir = std::env::temp_dir().join(format!("koolade_tab_isolation_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    if let Screen::Connected(p) = &mut app.screen {
        p.chat = vec![ChatMessage::new(
            ChatRole::Agent,
            "MAIN HISTORY MARKER",
            None,
        )];
        let mut saved = crate::persistence::task_chats::TaskChats::default();
        saved
            .append(
                slug,
                first,
                vec![ChatMessage::new(
                    ChatRole::Agent,
                    "FIRST TASK HISTORY",
                    None,
                )],
            )
            .unwrap();
        saved
            .append(
                slug,
                second,
                vec![ChatMessage::new(
                    ChatRole::Agent,
                    "SECOND TASK HISTORY",
                    None,
                )],
            )
            .unwrap();
        p.task_chats = Default::default();
        p.task_chats.ensure_loaded(slug);
    }
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Open conversation");
    // Add the second loaded conversation to the same tab strip, then exercise
    // actual pointer-driven tab selection and closing below.
    ctx.data_mut(|d| {
        let id = egui::Id::new("koolade_chat_tabs");
        let mut tabs = d.get_temp::<crate::ui::layout::ChatTabs>(id).unwrap();
        tabs.keys.push(second.into());
        d.insert_temp(id, tabs);
    });
    let assert_history = |output: &egui::FullOutput, expected_key: &str| {
        assert!(text_position(output, "MAIN HISTORY MARKER").is_none());
        // Both histories may be previewed inline on their own board items. The
        // tab's active key controls which one is opened in the conversation pane.
        assert!(text_position(output, "FIRST TASK HISTORY").is_some());
        assert!(text_position(output, "SECOND TASK HISTORY").is_some());
        let first_history = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if matches!(
                    text.galley.text(),
                    "FIRST TASK HISTORY" | "SECOND TASK HISTORY"
                ) =>
            {
                Some(text.galley.text())
            }
            _ => None,
        });
        let expected_marker = if expected_key == first {
            "FIRST TASK HISTORY"
        } else {
            "SECOND TASK HISTORY"
        };
        assert_eq!(first_history, Some(expected_marker));
        ctx.data_mut(|d| {
            let tabs = d
                .get_temp::<crate::ui::layout::ChatTabs>(egui::Id::new("koolade_chat_tabs"))
                .unwrap();
            assert_eq!(tabs.active.as_deref(), Some(expected_key));
        });
    };
    let output = frame(&mut app, &ctx, vec![]);
    assert_history(&output, first);
    let label = text_position(&output, "TASK-001").unwrap();
    let close = text_position(&output, "×").unwrap();
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == crate::ui::theme::ACCENT_SOFT
            && rect.rect.contains(label) && rect.rect.contains(close))),
        "ID and close button must share one tab background"
    );
    let output = click_text(&mut app, &ctx, "TASK-002");
    assert_history(&output, second);
    let output = click_text(&mut app, &ctx, "TASK-001");
    assert_history(&output, first);
    click_text(&mut app, &ctx, "×");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "MAIN HISTORY MARKER").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
