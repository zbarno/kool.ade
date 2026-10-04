use super::*;

#[test]
fn board_is_primary_and_legacy_main_chat_is_hidden() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    *app.chat_draft() = "Keep my project draft".into();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let board = text_position(&output, "Board  3").unwrap();
    assert!(
        board.x < 200.0,
        "board starts at the primary workspace edge"
    );
    assert!(text_position(&output, "Main Chat").is_none());
    assert!(text_position(&output, "Keep my project draft").is_none());
    assert!(text_position(&output, "×").is_none());
    assert_eq!(output.viewport_output.len(), 1);
}

#[test]
fn conversation_tabs_focus_deduplicate_close_and_preserve_drafts() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    *app.chat_draft() = "Keep my project draft".into();
    *app.task_draft(key).unwrap() = "Task draft stays with this item".into();
    if let Screen::Connected(p) = &mut app.screen {
        p.task_chats.messages.insert(
            key.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                r#"{"assistant_message":"Task-only previous reply"}"#,
                None,
            )],
        );
    }
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Open conversation");
    let output = frame(&mut app, &ctx, vec![]);
    let tabs = || {
        ctx.data_mut(|d| {
            d.get_temp::<crate::ui::layout::ChatTabs>(egui::Id::new("koolade_chat_tabs"))
        })
        .unwrap()
    };
    assert_eq!(tabs().keys, vec![key.to_owned()]);
    assert_eq!(tabs().active.as_deref(), Some(key));
    assert!(text_position(&output, "Task draft stays with this item").is_some());
    assert!(text_position(&output, "Task-only previous reply").is_some());
    assert!(text_position(&output, "Keep my project draft").is_none());
    assert_eq!(output.viewport_output.len(), 1);
    click_text(&mut app, &ctx, "×");
    assert!(tabs().keys.is_empty());
    assert!(tabs().active.is_none());
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Keep my project draft").is_none());
    assert_eq!(
        app.task_draft(key).unwrap(),
        "Task draft stays with this item"
    );
    assert_eq!(app.chat_draft(), "Keep my project draft");
    click_text(&mut app, &ctx, "Open conversation");
    assert_eq!(tabs().active.as_deref(), Some(key));
}

#[test]
fn narrow_workspace_does_not_restore_main_chat_for_questions() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.task_documents.clear();
        p.state.items = vec![OpenItem::new(
            "CLR-050".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            "Which authentication provider?".into(),
            "Choose how users sign in.".into(),
        )];
    }
    let ctx = egui::Context::default();
    let size = egui::vec2(360.0, 480.0);
    frame_at(&mut app, &ctx, vec![], size);
    let output = frame_at(&mut app, &ctx, vec![], size);
    assert!(text_position(&output, "Main Chat").is_none());
    assert!(text_position(&output, "Board  1").is_some());
}

#[test]
fn narrow_task_workspace_leads_with_action_and_discloses_description() {
    let mut app = fixture();
    app.prepare_task_chat(".koolade-packet/planning/tasks/fixture/001-task.md");
    let ctx = egui::Context::default();
    let size = egui::vec2(360.0, 480.0);
    frame_at(&mut app, &ctx, vec![], size);
    click_text_at(&mut app, &ctx, "First task", size);
    let output = frame_at(&mut app, &ctx, vec![], size);
    let action = text_position(&output, "Implement & continue queue").unwrap();
    assert!(action.x > 0.0 && action.x < size.x && action.y > 0.0 && action.y < size.y);
    assert!(text_position(&output, "CURRENT STATE").is_some());
    assert!(text_position(&output, "YOUR NEXT STEP").is_some());
    assert!(
        text_position(&output, "Activity").is_some(),
        "{:?}",
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.text(), t.pos)),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(text_position(&output, "Unique story detail 0").is_none());
}

#[test]
fn document_switcher_displays_product_and_multiple_features_without_task_story_tab() {
    let mut app = fixture();
    let feature = |id: &str, title: &str, marker: &str| {
        let markdown = format!("# {id}: {title}\n\n{marker}\n");
        let identified =
            crate::domain::ArtifactIdentity::preserve_markdown(&markdown, None, id, title).unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
            .unwrap()
            .unwrap();
        crate::domain::ChangeMetadata::write_markdown(
            &identified,
            &identity,
            crate::domain::ChangeStatus::Draft,
        )
        .unwrap()
    };
    let first = feature("CHG-001", "First feature", "First proposal marker");
    let second = feature("CHG-002", "Second feature", "Second proposal marker");
    if let Screen::Connected(project) = &mut app.screen {
        project.state.spec_text = Some("# Product\n\nProduct behavior marker".into());
        project.state.active_feature = Some(("CHG-001".into(), first.clone()));
        project.state.active_features = vec![("CHG-001".into(), first), ("CHG-002".into(), second)];
    }
    let ctx = egui::Context::default();
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("koolade_document_tab"), false));
    frame(&mut app, &ctx, vec![]);
    let product = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&product, "Product behavior marker").is_some());
    assert!(text_position(&product, "Task Stories").is_none());
    assert!(text_position(&product, "First proposal marker").is_none());
    let first = click_text(&mut app, &ctx, "Features  2");
    assert!(text_position(&first, "First proposal marker").is_some());
    assert!(text_position(&first, "Product behavior marker").is_none());
    ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("koolade_selected_feature"),
            Some("CHG-001".to_string()),
        );
        data.insert_temp(
            egui::Id::new("koolade_selected_feature"),
            Some("CHG-002".to_string()),
        );
    });
    let second = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&second, "Second proposal marker").is_some());
    assert!(text_position(&second, "First proposal marker").is_none());
}
