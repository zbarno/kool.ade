use super::*;

#[test]
fn open_workspace_spawns_a_detached_sibling_without_touching_the_session() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);

    // Benign spawn target (unit-test pick of record: a trivial no-op
    // present on the Linux target hosts).
    const HARMLESS: [&str; 2] = ["/usr/bin/true", "/bin/true"];
    let target = HARMLESS
        .iter()
        .copied()
        .find(|cand| std::path::Path::new(cand).is_file())
        .unwrap_or_else(|| panic!("no trivial no-op utility on this Linux host"));
    app.spawn_target_override = Some(target.into());

    // Fake in-flight work: a real-but-suspended turn plus an active
    // implementation, alongside an unsent composer draft.
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md".to_string();
    let running = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture must be connected")
        };
        project.draft = "unsent draft must survive the sibling spawn".to_string();
        project.active_implementations.insert(
            ticket.clone(),
            crate::core::implementation::Controller::idle_fixture(),
        );
        std::rc::Rc::new(TurnController::start(
            crate::core::turn::TurnInputs {
                state: project.state.clone(),
                user_message: "Please continue".into(),
                recent_chat: Vec::new(),
                purpose: crate::core::workflow::TurnPurpose::Interview,
                comparison_feature: None,
            },
            Box::new(HangingTurnHarness),
        ))
    };
    {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture must be connected")
        };
        project.active_turn = Some(running.clone());
        project.live_progress = crate::harness::LiveProgress {
            activity: Some("Planning…".into()),
            ..Default::default()
        };
    }
    let draft_before = app.chat_draft().clone();

    // Click 1: Workspace → 'Open workspace'.
    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Open workspace");
    let output = frame_toasting(&mut app, &ctx);
    let painted = canvas_text(&output);
    assert!(
        painted.contains("Opening a new") && painted.contains("Kool.ad/e window"),
        "success toast expected, saw: {}",
        painted.chars().take(400).collect::<String>()
    );
    assert!(
        !painted.contains("Turn aborted"),
        "no 'Turn aborted' toast allowed"
    );
    assert!(
        matches!(app.screen, Screen::Connected(_)),
        "the invoking window must stay Connected"
    );
    assert_eq!(
        app.chat_draft(),
        &draft_before,
        "composer draft must be untouched"
    );
    {
        let Screen::Connected(project) = &app.screen else {
            panic!("fixture must be connected")
        };
        assert!(
            std::rc::Rc::ptr_eq(
                &running,
                project
                    .active_turn
                    .as_ref()
                    .unwrap_or_else(|| { panic!("the in-flight turn must still be registered") })
            ),
            "same controller still registered"
        );
        assert!(
            !running.cancel_requested(),
            "no cancel request may reach the turn"
        );
        assert!(
            !project
                .active_implementations
                .get(&ticket)
                .unwrap()
                .cancellation_requested(),
            "no cancel request may reach the implementations"
        );
    }

    // Click 2: repeat invocation spawns again with no shared-state
    // collision — the parent never waits on either Child.
    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Open workspace");
    let repainted = canvas_text(&frame_toasting(&mut app, &ctx));
    assert!(repainted.contains("Opening a new") && repainted.contains("Kool.ad/e window"));
    assert!(
        !running.cancel_requested(),
        "repeat click must not cancel either"
    );
    assert!(matches!(app.screen, Screen::Connected(_)));
    assert_eq!(app.chat_draft(), &draft_before);
}

#[test]
fn open_workspace_spawn_failure_warns_with_path_and_leaves_the_session_usable() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    if let Screen::Connected(project) = &mut app.screen {
        project.draft = "draft survives a failed spawn".to_string();
    }
    let missing = std::env::temp_dir().join(format!("koolade-sib-{}", std::process::id()));
    assert!(!missing.exists(), "test pre-condition");
    // Single word-group (no interior spaces), so it survives the toast's
    // soft line wrapping intact.
    let token = format!("koolade-sib-{}", std::process::id());
    app.spawn_target_override = Some(missing);

    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Open workspace");
    let output = frame_toasting(&mut app, &ctx);
    let painted = canvas_text(&output);
    assert!(
        painted.contains(&token),
        "warning toast must embed the failing binary path"
    );
    assert!(
        !(painted.contains("Opening a new") && painted.contains("Kool.ad/e window")),
        "no success toast on failure"
    );
    assert!(
        matches!(app.screen, Screen::Connected(_)),
        "screen stays Connected"
    );
    assert_eq!(app.chat_draft(), "draft survives a failed spawn");

    // Retry: the same graceful failure repeats (repeat-request stability,
    // no zombie half-interaction).
    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Open workspace");
    let output = frame_toasting(&mut app, &ctx);
    let painted = canvas_text(&output);
    assert!(
        painted.contains(&token),
        "retry warning must embed the path again"
    );
    assert!(
        !(painted.contains("Opening a new") && painted.contains("Kool.ad/e window")),
        "no success toast on failure retry"
    );
    assert!(matches!(app.screen, Screen::Connected(_)));
    assert_eq!(app.chat_draft(), "draft survives a failed spawn");
}

#[test]
fn settings_controls_open_in_modal_from_workspace_menu() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Build approved changes automatically").is_none());
    assert!(text_position(&output, "Plan automatically").is_none());
    assert!(text_position(&output, "Publish verified changes automatically").is_none());
    assert!(
        text_position(&output, "Live activity").unwrap().y
            < text_position(&output, "To do · 1").unwrap().y
    );
    assert!(text_position(&output, "Status").is_none());
    click_text(&mut app, &ctx, "Workspace");
    let output = click_text(&mut app, &ctx, "Settings…");
    assert!(text_position(&output, "Workspace settings").is_some());
    assert!(text_position(&output, "Build approved changes automatically").is_none());
    click_text(&mut app, &ctx, "Automation");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Build approved changes automatically").is_some());
    assert!(text_position(&output, "Plan automatically").is_some());
    assert!(text_position(&output, "Publish verified changes automatically").is_some());
    assert!(text_position(&output, "Wait for project checks before publishing").is_some());
    assert!(text_position(&output, "Concurrent tasks").is_some());
    let policy_repo = match &app.screen {
        Screen::Connected(project) => project.state.repo_root.clone(),
        Screen::Welcome => unreachable!(),
    };
    if let Screen::Connected(project) = &mut app.screen {
        project.investigation = Some(crate::core::investigation::Controller::idle_fixture(
            "CLR-981",
        ));
    }
    click_text(&mut app, &ctx, "Plan automatically");
    assert!(matches!(&app.screen, Screen::Connected(project)
        if !project.queue.auto_plan
            && project.investigation.as_ref().is_some_and(|run| run.cancellation_requested())));
    assert!(
        !crate::core::implementation_queue::Queue::load(&policy_repo)
            .unwrap()
            .auto_plan
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.investigation = None;
        let mut item = OpenItem::new(
            "CLR-981".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Can this be resolved from repository evidence?".into(),
            "Automatic planning should own this item.".into(),
        );
        item.authority = crate::domain::Authority::Agent;
        project.state.items.push(item);
    }
    app.advance_investigation();
    assert!(matches!(&app.screen, Screen::Connected(project)
        if !project.queue.auto_plan && project.investigation.is_none()));
    click_text(&mut app, &ctx, "Plan automatically");
    assert!(
        crate::core::implementation_queue::Queue::load(&policy_repo)
            .unwrap()
            .auto_plan
    );
    click_text(&mut app, &ctx, "Build approved changes automatically");
    assert!(matches!(&app.screen, Screen::Connected(project)
        if !project.queue.auto_build && !project.queue.auto_publish));
    let saved = crate::core::implementation_queue::Queue::load(&policy_repo).unwrap();
    assert!(!saved.auto_build && !saved.auto_publish);
    click_text(&mut app, &ctx, "Publish verified changes automatically");
    assert!(matches!(&app.screen, Screen::Connected(project)
        if !project.queue.auto_build && project.queue.auto_publish
            && project.queue.require_independent_checks));
    let saved = crate::core::implementation_queue::Queue::load(&policy_repo).unwrap();
    assert!(!saved.auto_build && saved.auto_publish && saved.require_independent_checks);
    assert!(
        crate::core::implementation_queue::Queue::load(&policy_repo)
            .unwrap()
            .require_independent_checks
    );
    click_text(&mut app, &ctx, "People & Stakeholders");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Who am I?").is_some());
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
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Workspace settings").is_none());
}

#[test]
fn workspace_settings_buttons_open_their_project_and_coding_destinations() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);

    click_text(&mut app, &ctx, "Workspace");
    let output = click_text(&mut app, &ctx, "Settings…");
    assert!(text_position(&output, "Workspace settings").is_some());

    let output = click_text(&mut app, &ctx, "Project & Git");
    assert!(text_position(&output, "Repository names").is_some());
    assert!(text_position(&output, "Registered repository names").is_some());

    let output = click_text(&mut app, &ctx, "Coding Tools");
    assert!(text_position(&output, "Available coding tools").is_some());
    let output = click_text(&mut app, &ctx, "Models & Routing");
    assert!(text_position(&output, "Implementation").is_some());
}

#[test]
fn narrow_or_short_workspace_settings_selects_a_page_from_compact_navigation() {
    for size in [egui::vec2(360.0, 480.0), egui::vec2(1280.0, 480.0)] {
        let mut app = fixture();
        let ctx = super::mockup_layout::styled_context();
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true)
        });
        frame_at(&mut app, &ctx, vec![], size);
        let general = frame_at(&mut app, &ctx, vec![], size);
        assert!(text_position(&general, "General").is_some());
        assert!(text_position(&general, "APPLICATION").is_none());
        click_text_at(&mut app, &ctx, "General", size);
        let appearance = click_text_at(&mut app, &ctx, "Appearance", size);
        assert!(text_position(&appearance, "Reduce motion").is_some());
    }
}

#[test]
fn workspace_repository_menu_uses_the_shared_display_label() {
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state.repositories.repositories[0].display_name =
            Some("Planning repository".into());
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Workspace");
    let output = click_text(&mut app, &ctx, "Registered repositories");
    assert!(text_contains(
        &output,
        "Open Planning repository in a new window"
    ));
}
