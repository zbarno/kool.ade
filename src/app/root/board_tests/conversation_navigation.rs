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
fn task_cards_open_persisted_conversation_in_details_and_keep_each_draft_isolated() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let key = "planning/tasks/fixture/001-task.md";
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
    let output = click_text(&mut app, &ctx, "First task");
    assert!(text_position(&output, "Task draft stays with this item").is_some());
    assert!(text_position(&output, "Task-only previous reply").is_some());
    assert!(text_position(&output, "Ask a question").is_some());
    assert!(text_position(&output, "Why am I seeing this?").is_some());
    assert!(text_position(&output, "Send").is_some());
    assert!(text_position(&output, "Keep my project draft").is_none());
    assert_eq!(output.viewport_output.len(), 1);
    assert_eq!(
        app.task_draft(key).unwrap(),
        "Task draft stays with this item"
    );
    assert_eq!(app.chat_draft(), "Keep my project draft");
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
    let second = "planning/tasks/fixture/002-task.md";
    *app.task_draft(second).unwrap() = "Second task draft".into();
    if let Screen::Connected(project) = &mut app.screen {
        project.task_chats.messages.insert(
            second.into(),
            vec![ChatMessage::new(ChatRole::Agent, "Second task only", None)],
        );
    }
    let output = click_text(&mut app, &ctx, "Review task");
    assert!(text_position(&output, "Second task only").is_some());
    assert!(text_position(&output, "Second task draft").is_some());
    assert!(text_position(&output, "Task-only previous reply").is_none());
    assert!(text_position(&output, "Ask a question").is_some());
}

#[test]
fn relationship_navigation_and_back_restore_each_tasks_own_chat_and_draft() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let first = "planning/tasks/fixture/001-task.md";
    let second = "planning/tasks/fixture/002-task.md";
    *app.task_draft(first).unwrap() = "Draft for original task".into();
    *app.task_draft(second).unwrap() = "Draft for related task".into();
    if let Screen::Connected(project) = &mut app.screen {
        project.task_chats.messages.insert(
            first.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Original task thread",
                None,
            )],
        );
        project.task_chats.messages.insert(
            second.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Related task thread",
                None,
            )],
        );
    }
    frame(&mut app, &ctx, vec![]);
    let original = click_text(&mut app, &ctx, "First task");
    assert!(text_contains(&original, "Original task thread"));
    assert!(text_contains(&original, "Draft for original task"));

    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_history"),
            vec![first.to_owned()],
        );
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_navigation"),
            second.to_owned(),
        );
    });
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| { data.get_temp::<String>(egui::Id::new("koolade_selected_task")) })
            .as_deref(),
        Some(second),
        "the board consumes the relationship navigation intent"
    );
    let related = frame(&mut app, &ctx, vec![]);
    let shown = related
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        text_contains(&related, "Related task thread"),
        "related task transcript should be scoped to its selected path: {shown:?}"
    );
    assert!(text_contains(&related, "Draft for related task"));
    assert!(!text_contains(&related, "Original task thread"));
    assert!(!text_contains(&related, "Draft for original task"));

    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_history"),
            Vec::<String>::new(),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_navigation"),
            first.to_owned(),
        );
    });
    frame(&mut app, &ctx, vec![]);
    let restored = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&restored, "Original task thread"));
    assert!(text_contains(&restored, "Draft for original task"));
    assert!(!text_contains(&restored, "Related task thread"));
    assert!(!text_contains(&restored, "Draft for related task"));
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
    app.prepare_task_chat("planning/tasks/fixture/001-task.md");
    let ctx = egui::Context::default();
    let size = egui::vec2(360.0, 480.0);
    frame_at(&mut app, &ctx, vec![], size);
    click_text_at(&mut app, &ctx, "First task", size);
    let output = frame_at(&mut app, &ctx, vec![], size);
    let action = text_position(&output, "Implement & continue queue")
        .expect("task implementation action is visible");
    assert!(action.x > 0.0 && action.x < size.x && action.y > 0.0 && action.y < size.y);
    assert!(text_position(&output, "CURRENT STATE").is_none());
    assert!(text_position(&output, "CURRENT STATE").is_none());
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

#[test]
fn selected_task_follows_its_uid_across_path_rename() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let old_path = "planning/tasks/fixture/001-task.md";
    let renamed_path = "planning/tasks/renamed/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        let mut identity = crate::domain::ArtifactIdentity::new("TASK-RENAME", "First task");
        identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
        let document = project
            .task_documents
            .iter_mut()
            .find(|document| document.path == old_path)
            .unwrap();
        document.identity = Some(identity);
        document.metadata = Some(metadata);
        project.bind_task_conversation_identities();
    }
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "First task");
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task_uid")))
            .is_some()
    );

    if let Screen::Connected(project) = &mut app.screen {
        let document = project
            .task_documents
            .iter_mut()
            .find(|document| document.path == old_path)
            .unwrap();
        document.path = renamed_path.into();
        project.bind_task_conversation_identities();
    }
    frame(&mut app, &ctx, vec![]);

    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(renamed_path)
    );
}
