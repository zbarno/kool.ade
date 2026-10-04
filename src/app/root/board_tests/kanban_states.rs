use super::*;

#[test]
fn migrated_open_item_remains_clickable_on_kanban() {
    let root = std::env::temp_dir().join(format!(
        "koolade_migrated_board_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(root.join("planning")).unwrap();
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Migrated product");
    std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
    let item = OpenItem::new(
        "CLR-041".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Who reviews saved searches?".into(),
        "Review ownership remains open.".into(),
    );
    std::fs::write(
        root.join("planning/open-items.md"),
        crate::artifacts::items_io::serialize(std::slice::from_ref(&item)),
    )
    .unwrap();
    crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.task_documents.clear();
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, &item.question).is_some());
    let details = click_text(&mut app, &ctx, &item.question);
    assert!(text_position(&details, &item.reason).is_some());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn kanban_distinguishes_authority_blockers_tasks_and_completed_work() {
    let mut app = fixture();
    let make = |id: &str, authority, priority, question: &str| {
        let mut item = OpenItem::new(
            id.into(),
            priority,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            question.into(),
            "Evidence".into(),
        );
        item.authority = authority;
        item
    };
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items = vec![
            make(
                "CLR-101",
                crate::domain::Authority::Human,
                crate::domain::Priority::Normal,
                "Human decision card",
            ),
            make(
                "CLR-102",
                crate::domain::Authority::Agent,
                crate::domain::Priority::High,
                "Agent resolving card",
            ),
            make(
                "CLR-103",
                crate::domain::Authority::Review,
                crate::domain::Priority::Normal,
                "Review decision card",
            ),
            make(
                "CLR-104",
                crate::domain::Authority::Human,
                crate::domain::Priority::Blocking,
                "Blocking human card",
            ),
        ];
    }
    let ctx = egui::Context::default();
    frame_at(&mut app, &ctx, vec![], egui::vec2(1800.0, 1500.0));
    let output = frame_at(&mut app, &ctx, vec![], egui::vec2(1800.0, 1500.0));
    for label in [
        "To do · 2",
        "In progress · 0",
        "In review · 1",
        "Needs attention · 3",
        "Done · 1",
        "Human decision card",
        "Agent resolving card",
        "Review decision card",
        "Blocking human card",
        "Human",
        "Agent",
        "Review",
        "Blocking",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    assert!(text_position(&output, "Non-actionable repository observation").is_none());
}

#[test]
fn agent_item_shows_live_investigation_on_board_and_in_detail() {
    let mut app = fixture();
    let mut item = OpenItem::new(
        "CLR-011".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Ambiguity,
        "General".into(),
        Some("All".into()),
        "Does the repository already persist queries?".into(),
        "Investigate the source.".into(),
    );
    item.authority = crate::domain::Authority::Agent;
    if let Screen::Connected(project) = &mut app.screen {
        project.task_documents.clear();
        project.state.items = vec![item.clone()];
        project.activity.tasks.insert(
            item.id.clone(),
            crate::harness::LiveProgress {
                activity: Some("Reading search source".into()),
                response: "Found the query cache but no persistence adapter".into(),
                thoughts: "Checking restart behavior".into(),
                ..Default::default()
            },
        );
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "To do · 1").is_some());
    assert!(text_position(&output, "Reading search source").is_none());
    let details = click_text(&mut app, &ctx, &item.question);
    assert!(text_position(&details, "Agent investigation").is_none());
    click_text(&mut app, &ctx, "Activity");
    let details = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&details, "Agent investigation").is_some());
    assert!(text_position(&details, "Worker notes").is_some());
    assert!(
        app.live_progress().is_none(),
        "Item worker output must stay out of main chat"
    );
}
